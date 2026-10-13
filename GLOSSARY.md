# Tavily Hikari

本项目通过代理服务处理 Tavily 请求，并以业务持久化保存事实、以独立运行时观测面支持长期诊断。这里的术语区分公开业务汇总、内部观测数据和外部观测平台，避免把它们当成同一类接口。

## Observability

**Runtime metric**:
由服务真实运行事件聚合产生的 Counter、Gauge 或 Histogram，用于描述服务健康、延迟、等待和资源趋势。
_Avoid_: 业务报表、日志全文、请求明细

**Exporter**:
把服务内部有界运行时指标提供给观测采集系统读取的只读接口。
_Avoid_: Public metrics API、日志导出器

**Operational dashboard**:
面向长期运行和故障诊断的观测展示，回答服务是否健康以及慢点、等待和重试位于哪里。
_Avoid_: 业务 dashboard、Public metrics API

**Source event**:
触发运行时指标更新的真实生命周期事件，例如请求完成、upstream 重试或 SQLite 等待结束。
_Avoid_: 业务数据库回填、定时随机样本

**Metric label**:
指标序列中用于固定分类的低基数字段；它只能使用显式允许的稳定枚举，不能携带请求或身份标识。
_Avoid_: 用户标签、请求标签

## Interfaces

**Public metrics API**:
现有面向公开展示的聚合 JSON 接口，只返回批准的业务汇总字段，不承担 Prometheus 抓取职责。
_Avoid_: Exporter、内部 metrics endpoint

**Observability backend**:
负责抓取、保存、查询或展示运行时指标的外部系统，例如 Prometheus、Grafana 或可选的 trace backend。
_Avoid_: 业务数据库、服务 exporter

**Freshness**:
观测数据距离最近一次成功采集或生成的时间状态，用于区分正常、stale、低样本和未知结果。
_Avoid_: HTTP health、业务数据更新时间

**Dashboard provisioning**:
由仓库文件声明并由 Grafana 自动加载 datasource、dashboard 和 alert rule 的配置方式。
_Avoid_: 手工 UI 配置、一次性导入

**Scrape credential**:
Prometheus 读取 exporter 时使用的只读认证材料，与应用侧或 tracing 凭据分离。
_Avoid_: public API token、Grafana 登录凭据
