//! Create-only controller deployment bootstrap.
//!
//! One command turns an empty directory into a deployment that starts: the trees exist with the
//! modes their material class calls for, a certificate authority and a controller identity are in
//! the secret tree, and a configuration names the rest relatively so the whole thing can be moved
//! without editing a line.
//!
//! It is create-only. A bootstrap that merged into an existing directory would have to decide what
//! to do about material already there, and every answer to that is worse than refusing.

use std::{
    error::Error,
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};

use cairn_layout::RuntimeTree;
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose,
};

use crate::ServerError;

/// Lays out one controller deployment under `directory`.
///
/// # Errors
///
/// Returns an error when the destination is not an empty directory, when certificate generation
/// fails, or when any file cannot be created.
pub fn run(
    directory: &Path,
    server_name: &str,
    control_address: &str,
    enrollment_address: &str,
) -> Result<(), ServerError> {
    let root = prepare_root(directory)?;
    for tree in RuntimeTree::CONTROLLER {
        create_tree(&root.join(tree.directory_name()), tree.mode())?;
    }
    let pki = generate_pki(server_name).map_err(|error| ServerError::Startup(error.to_string()))?;
    let secrets = root.join(RuntimeTree::Secrets.directory_name());
    write_public(&secrets.join("ca.pem"), pki.ca_certificate.as_bytes())?;
    crate::write_new_secret_file(&secrets.join("ca-key.pem"), pki.ca_private_key.as_bytes())?;
    write_public(
        &secrets.join("server.pem"),
        pki.server_certificate.as_bytes(),
    )?;
    crate::write_new_secret_file(
        &secrets.join("server-key.pem"),
        pki.server_private_key.as_bytes(),
    )?;

    // Configuration is one of the seven trees, so it goes in that tree rather than loose at the
    // deployment root. Paths inside it resolve against the file's own directory, so `store` and the
    // rest still name the trees beside it.
    let config_path = root
        .join(RuntimeTree::Config.directory_name())
        .join("server.json");
    let config = configuration(server_name, control_address, enrollment_address)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    let mut bytes = serde_json::to_vec_pretty(&config)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    bytes.push(b'\n');
    crate::write_new_secret_file(&config_path, &bytes)?;

    // The migration product configuration is generated for the same reason the server's is: a
    // deployment whose configuration is hand-copied from an example is a deployment whose values
    // nothing checks, and this one shipped a coverage policy naming a strategy role the product
    // does not implement. Generating it means the example is exercised on every bootstrap.
    let migration_path = root
        .join(RuntimeTree::Config.directory_name())
        .join("migration.json");
    let migration =
        migration_configuration().map_err(|error| ServerError::Startup(error.to_string()))?;
    let mut migration_bytes = serde_json::to_vec_pretty(&migration)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    migration_bytes.push(b'\n');
    crate::write_new_secret_file(&migration_path, &migration_bytes)?;

    // The trees are named here rather than left to be discovered, because the next thing an
    // operator does is decide where their own material goes.
    println!("Created a server deployment at {}", root.display());
    for tree in RuntimeTree::CONTROLLER {
        println!("  {}/  mode {:o}", tree.directory_name(), tree.mode());
    }
    println!("  {}/server.json", RuntimeTree::Config.directory_name());
    println!("  {}/migration.json", RuntimeTree::Config.directory_name());
    println!();
    println!("The certificate authority in secrets/ is self-signed and names {server_name}.");
    println!("Replace it with your own before this deployment issues credentials you rely on.");
    println!();
    println!("The addresses given are used both to listen and to advertise. A deployment that");
    println!("reaches the controller through a tunnel or a proxy sees different ones, and the");
    println!("advertised pair lives in enrollment_service.public_tcp_address and");
    println!("enrollment_service.control_endpoint.tcp_address.");
    println!();
    println!("Start it with:");
    println!("  cairn-server {}", config_path.display());
    println!("or, for the CUDA migration product, with:");
    println!("  cairn-cuda-migration-server {}", migration_path.display());
    Ok(())
}

struct GeneratedPki {
    ca_certificate: String,
    ca_private_key: String,
    server_certificate: String,
    server_private_key: String,
}

fn generate_pki(server_name: &str) -> Result<GeneratedPki, Box<dyn Error>> {
    let ca_key = KeyPair::generate()?;
    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, format!("Cairn CA for {server_name}"));
    ca_params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    let ca = ca_params.self_signed(&ca_key)?;
    let server_key = KeyPair::generate()?;
    let mut server_params = CertificateParams::new(vec![server_name.to_owned()])?;
    server_params
        .distinguished_name
        .push(DnType::CommonName, server_name);
    server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let server = server_params.signed_by(&server_key, &ca, &ca_key)?;
    Ok(GeneratedPki {
        ca_certificate: ca.pem(),
        ca_private_key: ca_key.serialize_pem(),
        server_certificate: server.pem(),
        server_private_key: server_key.serialize_pem(),
    })
}

/// Returns the documented migration product configuration.
///
/// It is shipped verbatim rather than filled in, because every value it carries is a deployment
/// policy choice and none of them is derivable from the arguments a bootstrap is given. What this
/// buys is the same thing generating the server's configuration buys: the example stops being
/// something an operator copies unchecked, and a value that no longer works stops being something
/// only a live run discovers.
fn migration_configuration() -> Result<serde_json::Value, Box<dyn Error>> {
    Ok(serde_json::from_str(include_str!(
        "../../../config/migration.example.json"
    ))?)
}

/// Fills the documented configuration in, so the example and what bootstrap writes cannot drift:
/// if the example stops decoding, this stops working.
fn configuration(
    server_name: &str,
    control_address: &str,
    enrollment_address: &str,
) -> Result<serde_json::Value, Box<dyn Error>> {
    let mut config: serde_json::Value =
        serde_json::from_str(include_str!("../../../config/server.example.json"))?;
    config["listen"] = serde_json::json!(control_address);
    let service = config
        .get_mut("enrollment_service")
        .ok_or("documented configuration has no enrollment service")?;
    service["listen"] = serde_json::json!(enrollment_address);
    service["public_tcp_address"] = serde_json::json!(enrollment_address);
    service["server_name"] = serde_json::json!(server_name);
    service["websocket_uri"] = serde_json::json!(format!(
        "wss://{server_name}:{}/enrollment",
        port(enrollment_address)?
    ));
    let endpoint = service
        .get_mut("control_endpoint")
        .ok_or("documented configuration has no control endpoint")?;
    endpoint["tcp_address"] = serde_json::json!(control_address);
    endpoint["server_name"] = serde_json::json!(server_name);
    endpoint["websocket_uri"] = serde_json::json!(format!(
        "wss://{server_name}:{}/control",
        port(control_address)?
    ));
    Ok(config)
}

fn port(address: &str) -> Result<&str, Box<dyn Error>> {
    address
        .rsplit_once(':')
        .map(|(_, port)| port)
        .ok_or_else(|| format!("{address} does not name a port").into())
}

fn prepare_root(directory: &Path) -> Result<PathBuf, ServerError> {
    let configuration = |error: String| ServerError::Configuration(error);
    if directory.exists() {
        if !directory.is_dir() {
            return Err(configuration(format!(
                "{} exists and is not a directory",
                directory.display()
            )));
        }
        let empty = fs::read_dir(directory)
            .map_err(|error| configuration(error.to_string()))?
            .next()
            .is_none();
        if !empty {
            return Err(configuration(format!(
                "{} is not empty; bootstrap never merges into an existing deployment",
                directory.display()
            )));
        }
    } else {
        fs::create_dir_all(directory).map_err(|error| configuration(error.to_string()))?;
    }
    fs::canonicalize(directory).map_err(|error| configuration(error.to_string()))
}

fn create_tree(path: &Path, mode: u32) -> Result<(), ServerError> {
    fs::create_dir_all(path).map_err(|error| ServerError::Configuration(error.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|error| ServerError::Configuration(error.to_string()))?;
    }
    Ok(())
}

fn write_public(path: &Path, bytes: &[u8]) -> Result<(), ServerError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o644);
    }
    let mut file = options
        .open(path)
        .map_err(|error| ServerError::Configuration(error.to_string()))?;
    file.write_all(bytes)
        .map_err(|error| ServerError::Configuration(error.to_string()))
}
