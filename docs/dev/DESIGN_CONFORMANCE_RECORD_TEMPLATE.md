# Development Slice DesignConformanceRecord 模板

- 状态：规范性计划模板
- 日期：2026-08-27
- 使用规则：每个代码 slice 在进入 `Ready` 前复制并填写；不得仅链接 PR 描述替代

## 1. Identity

- Development Slice ID：
- Title：
- Stage / Integration Increment：
- Owner role：
- Review roles：
- Proposed status：
- Target branch/change set：

## 2. Objective 与非目标

- 单一 objective：
- 用户可观察或架构可验证的结果：
- 明确非目标：
- 完成后仍 unknown/not-executed 的 scope：

## 3. Traceability

- Requirement IDs：
- Decision IDs：
- System Design sections：
- Focused design sections：
- Open questions/blockers：
- 被本 slice supersede/remove 的旧开发路径：

## 4. Domain 与 Authority

| 项目 | Exact type/process/port | Authority | 不允许的能力 |
| --- | --- | --- | --- |
| Applicant/proposal | | | |
| Required obligations | | | |
| Planner/strategy | | | |
| Execution observation | | | |
| Mechanical Gate | | | |
| Admitted/output artifact | | | |
| Record/store | | | |

## 5. Strong types 与 V1 影响

- 新增/修改 identities：
- 容易混淆的 types/units/states：
- constructor/deserialization invariants：
- compile-fail/static boundary tests：
- V1 schemas/events/content domains/process protocols：
- 被删除的 V1 code/tests/fixtures：
- 确认无 version bump、alias、dual reader/writer、converter：

## 6. Inputs、Context 与 Data Policy

- Frozen public inputs：
- Restricted inputs：
- Secret/external references：
- Model/tool-visible projection：
- Knowledge/skill snapshot：
- Previous feedback 与 contamination：
- Hidden corpus/exposure policy：
- 禁止进入该 role/process 的数据：

## 7. Mechanisms 与 Qualification

| Mechanism | Exact identity | Qualification evidence | Scope/limitations | Requalification trigger |
| --- | --- | --- | --- | --- |
| | | | | |

## 8. Effects、Receipts 与 Recovery

- External effects：
- Authorization point：
- Ambiguous-effect policy：
- Authoritative receipts：
- Commit/publish sequence：
- Crash/restart points：
- Retry/idempotency identity：
- Cancellation/suspension：

## 9. Controls 与 Evidence Lanes

| Control/lane | Required? | Fixture/environment | Expected outcome | Evidence location |
| --- | --- | --- | --- | --- |
| Positive | | | | |
| Negative | | | | |
| Conflict | | | | |
| Unknown | | | | |
| Bypass/tamper | | | | |
| Fault/restart | | | | |
| Recorded workflow | | | | |
| Live model | | | | |
| CUDA | | | | |
| Ascend build | | | | |
| Ascend NPU | | | | |
| Profiler/microbench | | | | |
| Model integration | | | | |

## 10. Budgets 与停止

- Turn/token/tool/wall/external budgets：
- CPU/CUDA/NPU budget：
- Diagnostic/hidden exposure budget：
- Stop/saturation policy：
- Budget exhaustion outcome：

## 11. Backwards Audit 与 Impact

- Output → input/evidence edges：
- Common dependencies：
- Feedback/knowledge/skill visibility edges：
- Retraction/refutation impact：
- Recorded replay scope：
- Live counterfactual changed variables：

## 12. Acceptance

- Slice-specific exit criteria：
- Applicable G0–G6 gates：
- Required repository checks：
- Required external lanes：
- Documentation updates：
- Fact-ledger update：

## 13. Final Review（完成时填写）

- Accepted commit(s)：
- Evidence identities/paths：
- 与初始设计的偏差：
- Remaining gaps：
- Revalidation triggers：
- Final status：
