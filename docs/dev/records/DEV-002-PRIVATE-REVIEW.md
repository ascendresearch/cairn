# DEV-002 independent qualification-control review

- 状态：`Accepted`
- Parent record：[`DEV-002.md`](DEV-002.md)
- Decision：[`D-040`](../../DECISIONS.md#d-040--the-first-verifier-qualification-set-is-deterministic-and-intent-scoped)
- Public implementation commit：`a713d0059c331fef8da188b02f2c5854a39a9980`
- Reviewer：`qualification-reviewer-user`
- Outcome：四项检查全部接受（用户于2026-08-28明确attest）

## 1. Review purpose and authority

本审查位于qualification-control authoring和DEV-002 freeze之间。它确认exact public exam与private
wrong-binding/redaction controls适合作为future DEV-100/102/103/104 mechanism qualification输入；不执行
qualification，不产生任何mechanism implementation identity/receipt，也不执行SIR或Intent Admission。

Reviewer必须没有创作被审查的exact public control expectations或private control bytes，具有private root的
本地只读权限，并实际检查两侧bytes。机械audit、作者自审、只读public summary或模型agreement都不能生成
review authority。Reviewer不得直接修改control；任何public/private bytes变化都产生新identity并要求从头审查。

## 2. Exact local inputs

- public bundle：`fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1/`，review-pending bundle identity
  `cairn:v1:sha256:testkit.intent-qualification-public-bundle.v1:273b563a9a2c3d19438d4d96ef1d1fa42660e589e00692dd76f6cbf98e1c3246`；
- exact public review subject：
  `cairn:v1:sha256:testkit.intent-qualification-review-subject.v1:1ffdeef88592e8fa50991f6a03c9d870449d148a246c7e0a6fe3aa084e89af11`；
- private control root：`.cairn/secrets/restricted-corpora/qualification/intent/reduce-sum-f32/v1/`；
- private `manifest.json`只包含三类control facts、exact private identities和sealed exposure policy；它不包含
  review outcome；
- public `restricted-controls.public.json`在receipt生成前必须保持三类`review-pending`且receipt identity为空；
- private `review-receipt.json`在reviewer明确接受前必须不存在。

Private bytes、control/manifest identities、bindings、canary values和distinguishing diagnostics不得复制到Git、
聊天、普通日志或public review comment。开始前确认`git ls-files .cairn`为空。

## 3. Required review checks

Reviewer必须对同一exact public review subject和private manifest确认全部四项：

1. `golden-independence`：十项public expected behavior来自D-039/D-040、DEV-001 exact inputs和独立语义规则，
   不是从待测mechanism输出、旧generic comparator结果或作者预填的qualification verdict反推；public contract
   中不存在implementation identity或qualification receipt字段；
2. `wrong-binding-validity`：private wrong-binding control确实改变至少一项verdict-relevant binding，不能从
   applicant-visible public值推导正确私有binding，且未来closure/Gate必须fail closed；
3. `redaction-canary-validity`：hidden与synthetic-secret两类canary能够分别暴露distinguishing disclosure和
   secret disclosure；它们不是换名重复，也不会把真实credential带入fixture；
4. `exposure-and-diagnostic-safety`：private bytes/identities未公开，public summary只暴露固定category/status和
   后续redacted receipt identity；未来diagnostic不得泄漏private value、identity或binding，也不得通过差异输出
   间接区分canary。

Public slot/control sanity questions：

| Subject | Reviewer必须确认 |
| --- | --- |
| 十项mechanism slots | 恰好覆盖D-040十项，未合并、删项或以future implementation pending为理由跳过 |
| goldens与perturbations | 每项至少有honest control和针对该mechanism边界的mutation/fault，而非generic pass/fail占位 |
| review/requalification | mechanism owner不是sole reviewer；changed source/dependency/toolchain/environment/limitation会触发重审 |
| authority boundary | control-review receipt不能冒充mechanism qualification receipt，fixture不能构造admitted outcome |

## 4. Outcome protocol

若任一项失败，outcome是`RejectedForRevision`。Reviewer只通过private channel把必要修改交给control author；
public ledger只记录未通过的check category，不公开control-specific diagnosis。changed bytes使旧public review
subject或private manifest identity失效，机械audit和全部四项review重新开始。

若全部通过，Reviewer声明一个canonical `qualification-reviewer-*` identity，并明确attest其审查了本文件列出的
exact public review subject和exact local private manifest。随后由strict tooling生成private
`IntentQualificationControlReviewReceiptV1`，receipt必须绑定：

- exact pre-receipt `IntentQualificationReviewSubjectIdentity`；
- exact private `RestrictedQualificationManifestId`；
- 不同强类型的control author与non-author reviewer identity；
- canonical四项check与三类private control；
- 唯一accepted outcome。

public summary只写入由receipt exact bytes派生的`QualificationControlReviewReceiptId`并把三类control切换为
`frozen-reviewed`。它不得公开private manifest/control identities。freeze-transition validator必须显式接收
reviewer实际检查的strongly typed private manifest identity，并证明除restricted-summary authority projection
外没有public artifact edge变化。

## 5. Acceptance boundary

Receipt生成后仍需重跑strict decode、exact public/private binding、freeze transition、public sanitation、
`.cairn`零tracked-file、workspace CI、Markdown link/secret/path scan和backwards audit。只有这些证据全部通过，
DEV-002才能从`InProgress`变为`Accepted`。该接受只冻结“考试和controls”；DEV-100/102/103/104仍必须对各自
exact mechanism implementation另行qualification，之后才能首次进入Intent Gate。

上述四项检查均已通过。Strict tooling已生成private review receipt，并完成只允许restricted-summary
authority projection变化的freeze transition；最终DEV-002状态仍需等待G1–G6和acceptance ledger闭合。

## 6. Completed receipt and freeze transition

- independently reviewed public review subject：
  `cairn:v1:sha256:testkit.intent-qualification-review-subject.v1:1ffdeef88592e8fa50991f6a03c9d870449d148a246c7e0a6fe3aa084e89af11`；
- redacted control-review receipt identity：
  `cairn:v1:sha256:testkit.qualification-control-review-receipt.v1:1b8b892807530fb47b5b4df1f65cf4a9df291932fed047164d346e1d565688b1`；
- frozen public bundle：
  `cairn:v1:sha256:testkit.intent-qualification-public-bundle.v1:acb24e22e011b2a57573f1e11c6c26e4cd63156605ce2fe0c7e9832e70a61acc`。

Private audit只返回三类control identity-bound和freeze transition通过的categorical result；private manifest/
control identities与bytes均未进入Git或public ledger。`IntentQualificationReviewSubjectIdentity`、
`IntentQualificationBundleIdentity`、`QualificationControlReviewReceiptId`和future mechanism qualification
receipt保持不同强类型。最终bundle只把public summary从`review-pending`切换为`frozen-reviewed`并写入
redacted receipt identity；其余public artifact edges与reviewer实际检查的subject完全相同。
