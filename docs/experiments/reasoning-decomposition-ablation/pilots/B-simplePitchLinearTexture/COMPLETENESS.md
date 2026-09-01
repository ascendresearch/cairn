# B pilot completeness audit

This audit was performed after reconstructing the run from the event store, content index, submitted
domain artifacts, product configuration, task request, administrator decision and current Git facts.

| Information class | Status | Evidence or limitation |
| --- | --- | --- |
| Task and source | Complete | Exact task ID, sample path and source bundle are known |
| Normal product entry | Complete | CLI/server/App API/workflow path was observed |
| Target context | Complete | Ascend 950PR (3510), CANN and environment are frozen in recovery input |
| Runtime model alias | Complete | `deepseek-v4-pro` |
| Exact provider/model deployment identity | Partial | Alias/catalog persisted; response-provider internals intentionally not copied |
| Exact executable build identity | Missing | HEAD known, dirty-worktree binary digest not recorded; formal-use blocker |
| Budgets and read limits | Complete | Recovered from `product.json` and episode-open events |
| SIR domain submission | Complete | Exported as `01-sir.json` |
| Administrator Intent decision | Complete | Exact admitted claim and contract identity recovered |
| Oracle dimension/item submissions | Complete through interruption | All proposal submissions exported in event order |
| Oracle item reviews | Complete through interruption | All 13 submitted reviews exported |
| Model private reasoning | Intentionally excluded | Not an experiment artifact and may contain sensitive model body |
| Tool rejection classes | Complete | 10 read-limit and 6 missing-schema rejections |
| Token usage | Complete as provider-reported | Totals and per-role aggregation recovered for 166 responses |
| Wall time | Complete | Durable event timestamps; active per-role seconds also reported |
| Monetary cost | Missing | No stable price snapshot/cost artifact was frozen |
| Worker experiment time | Complete | Zero proposal-visible experiments and zero Oracle controls for this task |
| Portfolio coherence Review | Not reached | Must not be interpreted as missing data from a completed run |
| Oracle controls/Admission | Not reached | Unsafe structural path discovered before execution |
| Candidate execution | Not reached by scope | Completion target was through Oracle Admission |
| Hidden evaluator outcome | Missing | No valid semantic evaluator existed; formal-use blocker |
| Applicability/duplication labels | Manual pilot assessment | Cross-dimension duplication and one non-candidate-facing accepted item documented |
| Protocol deviations | Complete | Prompt/tool changes and small read limit documented |
| Raw durable store | Available but transient | `/tmp/cairn-dogfood-v3`; not relied on as the sole preserved result |
| Submitted artifact integrity | Complete | 33 valid JSON files copied byte-for-byte from CAS; no secret-pattern hits |

## Reverse consistency checks

- 1 SIR submission + 3 item-set submissions + 3 item-set reviews + 13 item drafts + 13 item
  reviews = 33 exported domain artifacts.
- 13 drafts correspond to revision counts `3 + 4 + 3 + 2 + 1` for the five accepted items.
- 13 reviews contain 16 item findings and five final approvals.
- 34 opened episodes = 1 SIR + 3 discovery + 3 item-set Review + 14 Developer + 13 item Review.
- 33 completed episodes plus the interrupted third-item Developer episode = 34 opened episodes.
- Per-role response totals sum to 166; token columns sum to the global provider-reported totals.
- 192 completed + 16 rejected tool operations = 208 proposed tool calls.
- No portfolio submission, portfolio Review, control receipt, Admission outcome or Candidate artifact
  is claimed because none was reached.

## Audit verdict

No known B pilot conclusion has been omitted from `RESULT.md`. The remaining missing fields are
explicitly marked and are reasons to exclude this pilot from the formal paired comparison, not
facts to be imputed.
