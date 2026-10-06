# Dashboard translations

The dashboard ships in Chinese (default) and English (System → Settings →
Language). English source text is the key:

```tsx
import { t } from '../../i18n'

<Button>{t('Apply')}</Button>
toast(t('Using {name}', { name }))          // placeholders: {name}
aria-label={t('Filter nodes')}
```

Rules (enforced by `tools/test-i18n.cjs`, part of `npm test`):

- Call `t()` **only with a string literal** (single, double or a backtick
  literal without `${}`). Wrap strings where they are defined, e.g.
  `{ value: 'rule', label: t('Rule') }`, never `t(option.label)`.
- Variables go through placeholders: `t('{n} nodes', { n })`. The Chinese
  text must use the same placeholder names.
- Every literal needs an entry in `src/i18n/zh/<area>.ts`; unused entries fail
  the test. The same English text must translate the same way everywhere —
  shared words live in `zh/common.ts`.
- Do not translate: values sent to the agent, enum/ids, units (Mbps, dBm, MHz,
  MB), protocol and radio identifiers (LTE, NR, SA, NSA, PCI, EARFCN,
  NR-ARFCN, RSRP, RSRQ, SINR, RSSI, CQI, APN, IMEI, IMSI, ICCID, SSID, DHCP,
  DNS, TTL, TUN, PAC, HTTP, SOCKS5, USB, ECM, RNDIS, NCM, AT, CSV), product
  names (mihomo, ZTE, U60 Pro), error text returned by the agent or firmware.
- Translate visible text, `aria-label`, `title`, `placeholder`, toast and
  confirmation copy.
- Keep sentences whole; don't build a sentence from translated fragments. If
  JSX interleaves markup, translate each text run separately.
- Pure helpers may call `t()` too: tests run outside a browser, where `t()`
  returns the English text, so existing assertions stay valid.

## Style

- Concise, technical, calm; no exclamation marks. Full-width punctuation in
  Chinese sentences (，。：；（）), ASCII inside technical tokens.
- One space between Chinese and Latin letters or digits: `5G 信号`,
  `Wi-Fi 设置`, `{n} 个节点`, `剩余 {days} 天`.
- Buttons are verbs (应用、保存); headings are nouns.

## Glossary

| English | 中文 |
|---|---|
| Home / Signal / Network / Modem / Proxy / System | 首页 / 信号 / 网络 / 蜂窝 / 代理 / 系统 |
| Apply / Save / Cancel / Confirm / Retry / Refresh / Reset | 应用 / 保存 / 取消 / 确认 / 重试 / 刷新 / 重置 |
| Add / Edit / Delete / Update / Close / Start / Stop / Restart | 添加 / 编辑 / 删除 / 更新 / 关闭 / 启动 / 停止 / 重启 |
| Enable / Disable / Enabled / Disabled / On / Off | 启用 / 停用 / 已启用 / 已停用 / 开 / 关 |
| Unknown / Unavailable / Loading / Not reported | 未知 / 不可用 / 加载中 / 未上报 |
| Settings / Status / Details / Overview / Tools / Metrics | 设置 / 状态 / 详情 / 概览 / 工具 / 指标 |
| Signal strength / quality | 信号强度 / 信号质量 |
| Excellent / Good / Fair / Poor / No signal | 极好 / 良好 / 一般 / 较差 / 无信号 |
| Band / Band lock / Cell / Cell lock / Carrier | 频段 / 锁频段 / 小区 / 锁小区 / 载波 |
| Carrier aggregation / Primary cell / Secondary cell | 载波聚合 / 主小区 / 辅小区 |
| Network mode / Network type / Operator | 网络模式 / 网络类型 / 运营商 |
| Uplink / Downlink / Upload / Download / Throughput | 上行 / 下行 / 上传 / 下载 / 吞吐量 |
| Data usage / Reset day / Billing cycle | 流量统计 / 结算日 / 结算周期 |
| Clients / Connected devices / Hostname / MAC address | 终端 / 已连接设备 / 主机名 / MAC 地址 |
| Wi-Fi / Password / Hidden SSID / Channel / Bandwidth / Transmit power | Wi-Fi / 密码 / 隐藏 SSID / 信道 / 带宽 / 发射功率 |
| Guest network / Security | 访客网络 / 安全类型 |
| LAN / Gateway / Subnet mask / IP address | 局域网 / 网关 / 子网掩码 / IP 地址 |
| SMS / Inbox / Sent / Compose / Message | 短信 / 收件箱 / 已发送 / 写短信 / 消息 |
| Profile (APN) | 配置 |
| Battery / Charging / Charge limit / Temperature / Thermal | 电池 / 充电 / 充电上限 / 温度 / 温度 |
| Memory / Uptime / Process / Logger | 内存 / 运行时间 / 进程 / 记录器 |
| Reboot / Shut down / Factory reset | 重启 / 关机 / 恢复出厂设置 |
| Agent (the zte-agent service) | Agent（不译） |
| Device (the router) / Router | 设备 / 路由器 |
| Stale: “Showing the last … read. The latest refresh failed.” | 显示的是上次读取的…，最近一次刷新失败。 |
| Subscription / Node / Latency / Route / Mode / Rule / Global / Direct | 订阅 / 节点 / 延迟 / 线路 / 模式 / 规则 / 全局 / 直连 |
| Transparent proxy | 透明代理 |
