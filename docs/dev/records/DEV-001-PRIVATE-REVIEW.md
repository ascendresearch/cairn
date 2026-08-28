# DEV-001 independent private review

- 状态：`ReviewReady`
- Parent record：[`DEV-001.md`](DEV-001.md)
- Decision：[`D-039`](../../DECISIONS.md#d-039--the-first-intent-admission-operator-is-a-clean-room-finite-f32-reduction)
- Public implementation commit：`98c4681f783ffcc1f759ef1fd697725ef9c3990c`

## 1. Review purpose and authority

本审查位于private case authoring和DEV-001 freeze之间。它确认exact case set适合作为首次non-adaptive
held-out control input；不实现SIR、不执行Admission，也不证明CUDA/Ascend行为。

Reviewer必须没有创作被审查的exact case bytes，具有private root的本地只读权限，并实际检查bytes。模型
agreement、mechanical audit、作者自审或只阅读public summary均不能生成review authority。Reviewer不得直接
修改case；任何修改都产生新identity并要求从头审查。

## 2. Exact local inputs

- public bundle：`fixtures/cuda-ascend/intent/reduce-sum-f32/v1/`，bundle identity
  `cairn:v1:sha256:testkit.intent-public-bundle.v1:8eea4e384b52224b8ba1716bee38b23bd9fb102ed6578953bc1d4b89b473d1d8`；
- private case-set root：`.cairn/secrets/restricted-corpora/intent/reduce-sum-f32/v1/`；
- private `manifest.json`只包含case-set facts、exact private identities、D-039 reference和
  `sealed-until-disclosure` exposure policy；它不包含review outcome；
- public `restricted-partitions.public.json`在receipt生成前必须保持`review-pending`且receipt identity为空。

Private bytes、case identities、inputs、bindings和distinguishing diagnostics不得复制到Git、聊天、普通日志或
public review comment。开始前确认`git ls-files .cairn`为空。

## 3. Required review checks

Reviewer必须对同一exact public bundle和private manifest确认全部六项：

1. `clean-room-source-provenance`：CUDA/host source由Cairn从D-039重新创作，未复制provenance未清的
   Alloyport或第三方source bytes，MIT classification准确；
2. `d039-domain-and-abi`：public与private controls没有把empty、subnormal、non-finite、alias或wide shape
   偷渡进first domain，ABI仍为一个read-only input、一个single-element output和一个element count；
3. `partition-semantic-coverage`：六个case分别真实覆盖implementation artifact、source defect、deployment
   quirk、competing plausible meaning、genuine unknown和tamper/wrong binding，而不是换名重复；
4. `public-derivation-independence`：private case不是public case的复制或可从public exact values/diagnostics直接
   推导，case author没有把known expected bytes变成applicant-visible输入；
5. `binding-tamper-validity`：正常partition的required/presented binding一致，tamper partition确实wrong-bound，
   且失败应保持fail-closed；
6. `exposure-and-diagnostic-safety`：case bytes/identities未公开，未来只允许最小claim/outcome diagnostic；一旦
   distinguishing disclosure发生，该case必须burn为public regression。

Partition-specific semantic questions：

| Partition | Reviewer必须确认 |
| --- | --- |
| implementation artifact | control能阻止launch geometry/reduction tree被误升格为用户意图 |
| source defect | control能暴露实现缺陷，不能因source存在就把缺陷当intent |
| deployment quirk | control能阻止common shape或环境偶然性暗中缩窄domain |
| competing plausible meaning | alternative interpretation在证据前确实plausible，不能被预先当作错误答案 |
| genuine unknown | 信息不足时保持unknown，不在case中偷偷编码期望resolution |
| tamper/wrong binding | wrong applicant/policy/job/attempt/case binding不能借用其他receipt |

## 4. Outcome protocol

若任一项失败，outcome是`RejectedForRevision`。Reviewer只通过private channel把必要修改交给case author；public
ledger只记录未通过的check category，不公开case-specific diagnosis。changed bytes使旧manifest identity失效，
机械audit和全部六项review重新开始。

若全部通过，Reviewer给出一个canonical `private-reviewer-*` identity，并明确声明其审查了exact local public
bundle和private manifest。随后由strict tooling生成private `IntentPrivateReviewReceiptV1`，receipt必须绑定：

- exact public `IntentBundleIdentity`；
- exact private `RestrictedIntentManifestId`；
- case author与独立reviewer的不同强类型identity；
- D-039；
- canonical六项check与六个partition；
- 唯一accepted outcome。

public summary只写入由receipt exact bytes派生的`RestrictedReviewReceiptId`并把六个partition切换为
`frozen-reviewed`。它不得公开private manifest identity或case identities。

## 5. Acceptance boundary

Receipt生成后仍需重跑strict decode、identity binding、public sanitation、`.cairn`零tracked-file、workspace CI
和backwards audit。只有这些证据全部通过，DEV-001才能从`InProgress`变为`Accepted`；随后DEV-002才消费
frozen public bundle identity和redacted review receipt identity。
