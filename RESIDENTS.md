# RESIDENTS.md — 谱系居民清单与升级规程

> 内核是地基，本文件登记**所有运行在内核之上的居民项目**。
> **纪律：内核每打一个新 tag，维护者必须走一遍下表"升级确认"列——逐居民确认是否需要
> 及时升级，并把结论回填到本表与居民仓。** 这是谱系不散架的例行保养。

## 居民清单

| 居民 | 仓库 | 定位 | 当前内核依赖 | 居民版本 | 升级确认（2026-09-10） |
|------|------|------|-------------|---------|----------------------|
| react-agent | [zh2673-git/react-agent](https://github.com/zh2673-git/react-agent) | 全能 agent（ReAct + 工具 + 技能 + Web） | v0.1.7 | v0.1.17 | ✅ 已消费 v0.1.7（T8 泳道 / S1 trace 贯穿） |
| mini-agent | [zh2673-git/mini-agent](https://github.com/zh2673-git/mini-agent) | 最小问答 agent（谱系证明 P3） | v0.1.6 | v0.1.0 | ⚠️ 待升级至 v0.1.7（增量无破坏，升级 = 一行 tag + 测试） |

## 居民升级标准动作（三步，详见 PLAN「契约兼容策略」）

1. **升 tag**：居民仓的 `agent-kernel-*` 依赖 tag 升到内核新版本（一行）；
2. **跑验收**：居民全量回归测试 + 内核 `conformance` 出生证明套件全绿；
3. **记录**：居民 PLAN/CHANGELOG 记录，本表回填"升级确认"列与日期。

> 不升级的裁决也要记录（"已评估暂不升级"），防止悬而未决。

## 内核发版提醒清单（打 tag 前过一遍）

- [ ] `crates/Cargo.toml` workspace version 已同步（0.x 纪律：破坏性变更才进 y）
- [ ] `PLAN.md` 迭代记录已回填（目标/方案/P·Q·I/决策）
- [ ] invariants + conformance 全绿
- [ ] 本表"升级确认"列逐居民过一遍，确认谁需要跟进
- [ ] tag + push + GitHub Release（notes 按 P/Q/I 写）

## 弃用中的 API（居民迁移指引）

| API | 状态 | 替代 | 删除时点 |
|-----|------|------|---------|
| `HostApi::trace_id()` | deprecated（v0.1.3） | 直接用 `Envelope.trace_id` | 首个破坏性版本（0.2.0） |
| `Plugin::step()` | deprecated（v0.1.3） | 无（死钩子，内核从不调用） | 首个破坏性版本（0.2.0） |
