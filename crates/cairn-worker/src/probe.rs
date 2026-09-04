use std::{
    collections::BTreeSet,
    fs,
    num::NonZeroU64,
    path::{Path, PathBuf},
};

use cairn_execution::{
    AcceleratorDevice, AcceleratorDeviceCount, AcceleratorDeviceId,
    AcceleratorDiscoveryCompleteness, CapabilityName, CapabilityRequirement, CapabilityValue,
    LogicalCpuCount, MemoryByteCount, ResourceProbeVersion, ScratchByteCount,
    WorkerResourceObservation, WorkerResourceSource,
};
use cairn_protocol::ObservedAtUnixMillis;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const HOST_PROBE_VERSION: &str = "linux-host-resource-v1";

/// Built-in host-probe paths, freshness policy, and operator expectations.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceProbeConfig {
    pub scratch_path: PathBuf,
    /// `null` disables accelerator discovery and records a partial observation.
    pub accelerator_sysfs: Option<PathBuf>,
    /// `null` gives resource observations no time expiry during the worker incarnation.
    pub freshness_ms: Option<NonZeroU64>,
    /// `null` disables dynamic refresh; otherwise a fresh observation is sent at this interval.
    pub refresh_interval_ms: Option<NonZeroU64>,
    #[serde(default)]
    pub expected: ExpectedResourceConstraints,
}

/// Optional startup assertions over values produced by the built-in probe.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedResourceConstraints {
    pub minimum_logical_cpus: Option<LogicalCpuCount>,
    pub minimum_memory_bytes: Option<MemoryByteCount>,
    pub minimum_scratch_bytes: Option<ScratchByteCount>,
    pub minimum_accelerator_devices: Option<AcceleratorDeviceCount>,
    #[serde(default)]
    pub require_complete_accelerator_discovery: bool,
}

impl ExpectedResourceConstraints {
    fn validate(&self, observed: &WorkerResourceObservation) -> Result<(), ResourceProbeError> {
        if self
            .minimum_logical_cpus
            .is_some_and(|minimum| observed.logical_cpus() < minimum)
        {
            return Err(ResourceProbeError::ExpectedMismatch("logical CPUs"));
        }
        if self
            .minimum_memory_bytes
            .is_some_and(|minimum| observed.memory_bytes() < minimum)
        {
            return Err(ResourceProbeError::ExpectedMismatch("memory bytes"));
        }
        if self
            .minimum_scratch_bytes
            .is_some_and(|minimum| observed.scratch_available_bytes() < minimum)
        {
            return Err(ResourceProbeError::ExpectedMismatch("scratch bytes"));
        }
        if self.require_complete_accelerator_discovery
            && observed.accelerator_discovery() != AcceleratorDiscoveryCompleteness::Complete
        {
            return Err(ResourceProbeError::ExpectedMismatch(
                "accelerator discovery completeness",
            ));
        }
        if self.minimum_accelerator_devices.is_some_and(|minimum| {
            u64::try_from(observed.accelerators().len()).unwrap_or(u64::MAX) < minimum.get()
        }) {
            return Err(ResourceProbeError::ExpectedMismatch(
                "accelerator device count",
            ));
        }
        Ok(())
    }
}

/// Host filesystem, parsing, unit, overflow, or expectation failure.
#[derive(Debug, Error)]
pub enum ResourceProbeError {
    #[error("resource probe I/O failed: {0}")]
    Io(String),
    #[error("resource probe input has an invalid unit or shape: {0}")]
    InvalidInput(&'static str),
    #[error("resource probe quantity overflowed: {0}")]
    Overflow(&'static str),
    #[error("resource probe value is invalid: {0}")]
    Value(String),
    #[error("resource probe does not satisfy expected {0}")]
    ExpectedMismatch(&'static str),
    #[error("resource probe refresh interval must be shorter than its freshness lifetime")]
    InvalidRefreshPolicy,
}

/// Runs the supported Linux host resource probe once.
pub struct HostResourceProbe;

impl HostResourceProbe {
    /// Observes logical CPUs, physical memory, available local scratch, and generic accelerator
    /// sysfs devices. No operator-configured byte becomes an observed resource value.
    ///
    /// # Errors
    ///
    /// Fails closed for unavailable required host facts, invalid units, overflow, duplicate device
    /// facts, or configured expectation mismatch.
    pub fn probe(
        config: &ResourceProbeConfig,
        observed_at: ObservedAtUnixMillis,
    ) -> Result<WorkerResourceObservation, ResourceProbeError> {
        if config
            .refresh_interval_ms
            .zip(config.freshness_ms)
            .is_some_and(|(refresh, freshness)| refresh >= freshness)
        {
            return Err(ResourceProbeError::InvalidRefreshPolicy);
        }
        let logical = u64::try_from(
            std::thread::available_parallelism()
                .map_err(|error| ResourceProbeError::Io(error.to_string()))?
                .get(),
        )
        .map_err(|_| ResourceProbeError::Overflow("logical CPU count"))?;
        let logical_cpus = LogicalCpuCount::new(logical)
            .map_err(|error| ResourceProbeError::Value(error.to_string()))?;
        // The path is part of the reason. A probe that reads several paths and reports only
        // "no such file or directory" leaves an operator to guess which one, and this one has
        // already cost a deployment that guess.
        let memory_wire = fs::read_to_string("/proc/meminfo")
            .map_err(|error| ResourceProbeError::Io(format!("/proc/meminfo: {error}")))?;
        let memory_bytes = parse_memory_bytes(&memory_wire)?;
        let stat = rustix::fs::statvfs(&config.scratch_path).map_err(|error| {
            ResourceProbeError::Io(format!("{}: {error}", config.scratch_path.display()))
        })?;
        let scratch = stat
            .f_frsize
            .checked_mul(stat.f_bavail)
            .ok_or(ResourceProbeError::Overflow("scratch bytes"))?;
        let scratch_available_bytes = ScratchByteCount::new(scratch)
            .map_err(|error| ResourceProbeError::Value(error.to_string()))?;
        let (accelerator_discovery, accelerators) =
            discover_accelerators(config.accelerator_sysfs.as_deref())?;
        let valid_until = config
            .freshness_ms
            .map(|duration| freshness_deadline(observed_at, duration))
            .transpose()?;
        let observed = WorkerResourceObservation::new(
            WorkerResourceSource::BuiltinProbe,
            ResourceProbeVersion::new(HOST_PROBE_VERSION)
                .map_err(|error| ResourceProbeError::Value(error.to_string()))?,
            observed_at,
            valid_until,
            logical_cpus,
            memory_bytes,
            scratch_available_bytes,
            accelerator_discovery,
            accelerators,
        )
        .map_err(|error| ResourceProbeError::Value(error.to_string()))?;
        config.expected.validate(&observed)?;
        Ok(observed)
    }
}

fn parse_memory_bytes(input: &str) -> Result<MemoryByteCount, ResourceProbeError> {
    let line = input
        .lines()
        .find(|line| line.starts_with("MemTotal:"))
        .ok_or(ResourceProbeError::InvalidInput("missing MemTotal"))?;
    let mut fields = line.split_ascii_whitespace();
    if fields.next() != Some("MemTotal:") {
        return Err(ResourceProbeError::InvalidInput("invalid MemTotal label"));
    }
    let kibibytes = fields
        .next()
        .ok_or(ResourceProbeError::InvalidInput("missing MemTotal value"))?
        .parse::<u64>()
        .map_err(|_| ResourceProbeError::InvalidInput("invalid MemTotal value"))?;
    if fields.next() != Some("kB") || fields.next().is_some() {
        return Err(ResourceProbeError::InvalidInput(
            "MemTotal must use the kernel kB unit",
        ));
    }
    let bytes = kibibytes
        .checked_mul(1024)
        .ok_or(ResourceProbeError::Overflow("memory bytes"))?;
    MemoryByteCount::new(bytes).map_err(|error| ResourceProbeError::Value(error.to_string()))
}

fn discover_accelerators(
    root: Option<&Path>,
) -> Result<(AcceleratorDiscoveryCompleteness, Vec<AcceleratorDevice>), ResourceProbeError> {
    let Some(root) = root else {
        return Ok((AcceleratorDiscoveryCompleteness::Partial, Vec::new()));
    };
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((AcceleratorDiscoveryCompleteness::Complete, Vec::new()));
        }
        Err(error) => {
            return Err(ResourceProbeError::Io(format!(
                "{}: {error}",
                root.display()
            )));
        }
    };
    let mut complete = true;
    let mut devices = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| ResourceProbeError::Io(error.to_string()))?;
        let device_id = entry
            .file_name()
            .into_string()
            .map_err(|_| ResourceProbeError::InvalidInput("non-UTF-8 accelerator name"))?;
        let capabilities = if let Ok(value) = fs::read_to_string(entry.path().join("device/uevent"))
        {
            parse_uevent(&value)?
        } else {
            complete = false;
            Vec::new()
        };
        devices.push(
            AcceleratorDevice::new(
                AcceleratorDeviceId::new(device_id)
                    .map_err(|error| ResourceProbeError::Value(error.to_string()))?,
                capabilities,
            )
            .map_err(|error| ResourceProbeError::Value(error.to_string()))?,
        );
    }
    devices.sort_by(|left, right| left.device_id().cmp(right.device_id()));
    if devices
        .windows(2)
        .any(|pair| pair[0].device_id() == pair[1].device_id())
    {
        return Err(ResourceProbeError::InvalidInput(
            "duplicate accelerator device",
        ));
    }
    Ok((
        if complete {
            AcceleratorDiscoveryCompleteness::Complete
        } else {
            AcceleratorDiscoveryCompleteness::Partial
        },
        devices,
    ))
}

fn parse_uevent(input: &str) -> Result<Vec<CapabilityRequirement>, ResourceProbeError> {
    let mut seen = BTreeSet::new();
    let mut capabilities = Vec::new();
    for line in input.lines() {
        let Some((key, value)) = line.split_once('=') else {
            return Err(ResourceProbeError::InvalidInput(
                "invalid accelerator uevent",
            ));
        };
        let name = match key {
            "DRIVER" => "driver",
            "PCI_ID" => "pci-id",
            "MODALIAS" => "modalias",
            _ => continue,
        };
        if !seen.insert(name) {
            return Err(ResourceProbeError::InvalidInput(
                "duplicate accelerator capability",
            ));
        }
        capabilities.push(CapabilityRequirement {
            name: CapabilityName::new(name)
                .map_err(|error| ResourceProbeError::Value(error.to_string()))?,
            value: CapabilityValue::new(value)
                .map_err(|error| ResourceProbeError::Value(error.to_string()))?,
        });
    }
    Ok(capabilities)
}

fn freshness_deadline(
    observed_at: ObservedAtUnixMillis,
    duration: NonZeroU64,
) -> Result<ObservedAtUnixMillis, ResourceProbeError> {
    let duration = i64::try_from(duration.get())
        .map_err(|_| ResourceProbeError::Overflow("freshness deadline"))?;
    observed_at
        .get()
        .checked_add(duration)
        .map(ObservedAtUnixMillis::new)
        .ok_or(ResourceProbeError::Overflow("freshness deadline"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn memory_parser_rejects_unit_mismatch_and_overflow() {
        assert_eq!(
            parse_memory_bytes("MemTotal: 1024 kB")
                .expect("memory")
                .get(),
            1_048_576
        );
        assert!(parse_memory_bytes("MemTotal: 1024 MB").is_err());
        assert!(parse_memory_bytes("MemTotal: 18014398509481984 kB").is_err());
    }

    #[test]
    fn accelerator_discovery_distinguishes_absent_partial_and_complete() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let absent = directory.path().join("absent");
        let (complete, devices) = discover_accelerators(Some(&absent)).expect("absent class");
        assert_eq!(complete, AcceleratorDiscoveryCompleteness::Complete);
        assert!(devices.is_empty());

        let root = directory.path().join("accelerators");
        fs::create_dir_all(root.join("accel0/device")).expect("device directory");
        fs::write(
            root.join("accel0/device/uevent"),
            "DRIVER=fixture\nPCI_ID=1234:5678\n",
        )
        .expect("uevent");
        fs::create_dir_all(root.join("accel1")).expect("partial device");
        let (partial, devices) = discover_accelerators(Some(&root)).expect("partial class");
        assert_eq!(partial, AcceleratorDiscoveryCompleteness::Partial);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].capabilities().len(), 2);

        fs::create_dir_all(root.join("accel1/device")).expect("complete device");
        fs::write(root.join("accel1/device/uevent"), "DRIVER=fixture\n").expect("uevent");
        let (complete, devices) = discover_accelerators(Some(&root)).expect("complete class");
        assert_eq!(complete, AcceleratorDiscoveryCompleteness::Complete);
        assert_eq!(devices.len(), 2);
    }

    #[test]
    fn expected_constraints_fail_closed() {
        let observation = WorkerResourceObservation::new(
            WorkerResourceSource::BuiltinProbe,
            ResourceProbeVersion::new(HOST_PROBE_VERSION).expect("version"),
            ObservedAtUnixMillis::new(1),
            None,
            LogicalCpuCount::new(4).expect("CPUs"),
            MemoryByteCount::new(1024).expect("memory"),
            ScratchByteCount::new(2048).expect("scratch"),
            AcceleratorDiscoveryCompleteness::Partial,
            Vec::new(),
        )
        .expect("observation");
        let expected = ExpectedResourceConstraints {
            minimum_logical_cpus: Some(LogicalCpuCount::new(8).expect("CPUs")),
            ..ExpectedResourceConstraints::default()
        };
        assert!(expected.validate(&observation).is_err());
    }

    #[test]
    fn enabled_refresh_must_precede_enabled_expiry() {
        let config = ResourceProbeConfig {
            scratch_path: PathBuf::from("."),
            accelerator_sysfs: None,
            freshness_ms: NonZeroU64::new(100),
            refresh_interval_ms: NonZeroU64::new(100),
            expected: ExpectedResourceConstraints::default(),
        };
        assert!(matches!(
            HostResourceProbe::probe(&config, ObservedAtUnixMillis::new(0)),
            Err(ResourceProbeError::InvalidRefreshPolicy)
        ));
    }
}
