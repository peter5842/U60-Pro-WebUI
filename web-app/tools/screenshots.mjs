// Dashboard screenshots for the README, taken from the local demo (mock agent, no device):
//   bash tools/demo.sh &   # dashboard on 127.0.0.1:8080, mock agent on :9090
//   node tools/screenshots.mjs [outDir]
// The mock accepts any password; the UI language is Chinese (the browser default).
import { chromium } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { join } from 'node:path'

const out = process.argv[2] ?? '../docs/images'
const base = 'http://127.0.0.1:8080'
mkdirSync(out, { recursive: true })

const shots = [
  { file: 'home.png', nav: '首页' },
  { file: 'proxy.png', nav: '代理' },
  { file: 'proxy-nodes.png', nav: '代理', tab: '节点' },
  { file: 'network-ports.png', nav: '网络', tab: '端口' },
  { file: 'network-clients.png', nav: '网络', tab: '终端' },
  { file: 'modem-sms.png', nav: '蜂窝', tab: '短信' },
  { file: 'system.png', nav: '系统', tab: '设置' },
]

const browser = await chromium.launch()
const page = await browser.newPage({ viewport: { width: 1280, height: 860 }, locale: 'zh-CN', colorScheme: 'light' })
await page.goto(base)
await page.getByLabel('密码').fill('demo')
await page.getByRole('button', { name: '登录' }).click()
await page.getByRole('navigation').first().waitFor()

for (const s of shots) {
  await page.getByRole('navigation').getByRole('button', { name: s.nav, exact: true }).first().click()
  if (s.tab) await page.getByRole('tab', { name: s.tab, exact: true }).click()
  await page.waitForLoadState('networkidle')
  await page.waitForTimeout(1200)
  await page.screenshot({ path: join(out, s.file), fullPage: false })
  console.log('saved', s.file)
}
await browser.close()
