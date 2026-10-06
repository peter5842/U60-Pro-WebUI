/**
 * Browser regression harness: a synthetic agent behind Playwright request
 * interception. See tests/browser/README.md.
 *
 * Safety model (nothing here may reach hardware):
 *  - Every request the page makes goes through one `context.route` handler.
 *  - Requests to the local dev server origin pass through.
 *  - Requests to the agent origin (`http://127.0.0.1:9090`, the page's own
 *    hostname on the agent port) are answered by this module and NEVER
 *    forwarded. A method+path without a registered reply is recorded as
 *    "unhandled", answered with 501, and fails the test in teardown.
 *  - Everything else is aborted and recorded; a non-empty `blocked()` also
 *    fails the test in teardown.
 */
import { test as base, expect, type BrowserContext, type Route } from '@playwright/test'
import * as fx from './fixtures'

export { expect }

export interface AgentRequest {
  method: string
  path: string
  query: URLSearchParams
  body: unknown
  headers: Record<string, string>
}

export type Reply = { data: unknown; status?: number } | { error: string; status?: number }
export type ReplyArg = Reply | ((req: AgentRequest) => Reply | Promise<Reply>)

export interface Deferred {
  waitForRequest(): Promise<AgentRequest>
  resolve(data: unknown): void
  /** Respond with the JSON envelope `{ok:false,error}`. */
  reject(error: string, status?: number): void
  /** Network failure (fetch rejects). */
  abort(): void
}

export interface MockAgent {
  /** Replace the handler for method+path; persists until replaced. Also clears a `fail()` without `times`. */
  on(method: string, path: string, reply: ReplyArg): void
  /** Answer only the next matching request, then fall back to the `on` handler. */
  once(method: string, path: string, reply: ReplyArg): void
  /** Hold the NEXT matching request until settled. */
  defer(method: string, path: string): Deferred
  /** Fail matching requests. `times` omitted = until `on()` replaces it. Defaults: status 500, error 'synthetic failure'. */
  fail(method: string, path: string, opts?: { status?: number; error?: string; times?: number }): void
  requests(filter?: { method?: string; path?: string }): AgentRequest[]
  /** Non-GET requests except /api/auth/login and the read-only POST /api/sms/list. */
  mutations(): AgentRequest[]
  unhandled(): AgentRequest[]
  /** Extras. */
  /** Requests aborted because they targeted neither the dev server nor the agent origin. */
  blocked(): string[]
  /** Forget `blocked()` entries, for a test that deliberately provokes one. */
  acknowledgeBlocked(): void
  /** Forget recorded requests and unhandled entries (handlers stay). */
  clearRequests(): void
}

export const AGENT_ORIGIN = 'http://127.0.0.1:9090'
export const SYNTHETIC_TOKEN = 'synthetic-test-token'

const CORS = {
  'access-control-allow-origin': '*',
  'access-control-allow-methods': 'GET, POST, PUT, DELETE, OPTIONS',
  'access-control-allow-headers': 'Authorization, Content-Type, X-Confirm',
  'access-control-max-age': '86400',
}

const key = (method: string, path: string) => `${method.toUpperCase()} ${path}`

/** Read-only POST endpoints: not counted by mutations(). */
const READ_ONLY_POSTS = new Set(['/api/sms/list'])

interface Override {
  reply: ReplyArg
  remaining: number
  persistentFail: boolean
}

class DeferredImpl implements Deferred {
  private requestWaiters: Array<(r: AgentRequest) => void> = []
  private seen: AgentRequest | undefined
  private settle!: (v: Reply | 'abort') => void
  readonly settled = new Promise<Reply | 'abort'>((res) => {
    this.settle = res
  })

  attach(req: AgentRequest) {
    this.seen = req
    for (const w of this.requestWaiters) w(req)
    this.requestWaiters = []
  }
  waitForRequest() {
    if (this.seen) return Promise.resolve(this.seen)
    return new Promise<AgentRequest>((res) => this.requestWaiters.push(res))
  }
  resolve(data: unknown) {
    this.settle({ data })
  }
  reject(error: string, status = 500) {
    this.settle({ error, status })
  }
  abort() {
    this.settle('abort')
  }
}

export class AgentImpl implements MockAgent {
  private handlers = new Map<string, ReplyArg>()
  private overrides = new Map<string, Override[]>()
  private deferreds = new Map<string, DeferredImpl[]>()
  private log: AgentRequest[] = []
  private unhandledLog: AgentRequest[] = []
  private blockedLog: string[] = []
  private pending = new Set<DeferredImpl>()

  on(method: string, path: string, reply: ReplyArg) {
    const k = key(method, path)
    this.handlers.set(k, reply)
    const kept = (this.overrides.get(k) ?? []).filter((o) => !o.persistentFail)
    this.overrides.set(k, kept)
  }
  once(method: string, path: string, reply: ReplyArg) {
    this.push(key(method, path), { reply, remaining: 1, persistentFail: false })
  }
  defer(method: string, path: string): Deferred {
    const d = new DeferredImpl()
    const k = key(method, path)
    this.deferreds.set(k, [...(this.deferreds.get(k) ?? []), d])
    this.pending.add(d)
    return d
  }
  fail(method: string, path: string, opts: { status?: number; error?: string; times?: number } = {}) {
    const reply: Reply = { error: opts.error ?? 'synthetic failure', status: opts.status ?? 500 }
    this.push(key(method, path), {
      reply,
      remaining: opts.times ?? Number.POSITIVE_INFINITY,
      persistentFail: opts.times === undefined,
    })
  }
  private push(k: string, o: Override) {
    this.overrides.set(k, [...(this.overrides.get(k) ?? []), o])
  }

  requests(filter?: { method?: string; path?: string }) {
    return this.log.filter(
      (r) => (!filter?.method || r.method === filter.method.toUpperCase()) && (!filter?.path || r.path === filter.path),
    )
  }
  mutations() {
    return this.log.filter(
      (r) => r.method !== 'GET' && r.method !== 'OPTIONS' && r.path !== '/api/auth/login' &&
        !(r.method === 'POST' && READ_ONLY_POSTS.has(r.path)),
    )
  }
  unhandled() {
    return [...this.unhandledLog]
  }
  blocked() {
    return [...this.blockedLog]
  }
  acknowledgeBlocked() {
    this.blockedLog = []
  }
  clearRequests() {
    this.log = []
    this.unhandledLog = []
  }

  /** @internal */
  noteBlocked(url: string) {
    this.blockedLog.push(url)
  }

  /** Abort every request still held by defer(); called at teardown. */
  releaseAll() {
    for (const d of this.pending) d.abort()
  }

  /** @internal Answer one request addressed to the agent origin. */
  async handle(route: Route) {
    const request = route.request()
    const url = new URL(request.url())
    const method = request.method().toUpperCase()

    if (method === 'OPTIONS') {
      await route.fulfill({ status: 204, headers: CORS })
      return
    }

    let body: unknown
    const raw = request.postData()
    if (raw != null) {
      try {
        body = JSON.parse(raw)
      } catch {
        body = raw
      }
    }
    const req: AgentRequest = { method, path: url.pathname, query: url.searchParams, body, headers: request.headers() }
    this.log.push(req)
    const k = key(method, req.path)

    const fulfil = async (reply: Reply) => {
      const status = reply.status ?? ('error' in reply ? 500 : 200)
      const json = 'error' in reply ? { ok: false, error: reply.error } : { ok: true, data: reply.data }
      await route.fulfill({ status, headers: { ...CORS, 'content-type': 'application/json' }, body: JSON.stringify(json) })
    }

    // 1. defer: hold the next matching request.
    const queue = this.deferreds.get(k)
    const held = queue?.shift()
    if (held) {
      held.attach(req)
      const outcome = await held.settled
      this.pending.delete(held)
      if (outcome === 'abort') await route.abort('failed')
      else await fulfil(outcome)
      return
    }

    // 2. once()/fail() overrides, in registration order.
    const ovs = this.overrides.get(k)
    if (ovs?.length) {
      const o = ovs[0]
      if (Number.isFinite(o.remaining)) {
        o.remaining -= 1
        if (o.remaining <= 0) ovs.shift()
      }
      await fulfil(await (typeof o.reply === 'function' ? o.reply(req) : o.reply))
      return
    }

    // 3. persistent handler.
    const h = this.handlers.get(k)
    if (h) {
      await fulfil(await (typeof h === 'function' ? h(req) : h))
      return
    }

    this.unhandledLog.push(req)
    await fulfil({ error: `unhandled by synthetic agent: ${k}`, status: 501 })
  }
}

/** Default GET (and read-only POST) fixtures: SA network, healthy device. */
export function registerDefaults(agent: MockAgent) {
  const ok = (data: unknown): Reply => ({ data })
  const get = (path: string, data: unknown | (() => unknown)) =>
    agent.on('GET', path, () => ok(typeof data === 'function' ? (data as () => unknown)() : data))
  get('/api/dashboard', () => fx.dashboard())
  get('/api/device', fx.device())
  get('/api/cpu', fx.cpu())
  get('/api/memory', fx.memory())
  get('/api/network/clients', fx.clients())
  get('/api/sim/info', fx.simInfo())
  get('/api/sim/imei', fx.simImei())
  get('/api/modem/capabilities', fx.modemCapabilities())
  get('/api/wifi/status', fx.wifiStatus())
  get('/api/router/dns', fx.dns())
  get('/api/router/lan', fx.lan())
  get('/api/device/thermal/all', fx.thermalAll())
  get('/api/device/battery-info', fx.batteryInfo())
  get('/api/device/battery/detail', fx.batteryDetail())
  get('/api/device/charger', fx.charger())
  get('/api/device/charge-control', fx.chargeControl())
  get('/api/router/apn/mode', fx.apnMode())
  get('/api/router/apn/profiles', fx.apnProfiles())
  get('/api/sms/capabilities', fx.smsCapabilities())
  get('/api/system/top', fx.top())
  get('/api/usb/status', fx.usbStatus())
  get('/api/ttl/status', fx.ttlStatus())
  get('/api/logger/signal/status', fx.loggerStatus())
  get('/api/logger/connection/status', fx.loggerStatus())
  get('/api/at/port', fx.atPort())
  // CSV downloads: the client accepts the JSON envelope form.
  get('/api/logger/signal/download', { csv: fx.signalLogCsv() })
  get('/api/logger/connection/download', { csv: fx.connectionLogCsv() })
  // Read-only POSTs and the login exchange (neither counts as a mutation).
  agent.on('POST', '/api/sms/list', ok(fx.smsList()))
  agent.on('POST', '/api/auth/login', ok({ token: SYNTHETIC_TOKEN }))
}

async function installRouting(context: BrowserContext, agent: AgentImpl, allowedOrigin: string) {
  await context.route('**/*', async (route) => {
    const url = route.request().url()
    let origin = ''
    try {
      origin = new URL(url).origin
    } catch {
      /* fall through to block */
    }
    try {
      if (origin === allowedOrigin) await route.fallback()
      else if (origin === AGENT_ORIGIN) await agent.handle(route)
      else {
        agent.noteBlocked(`${route.request().method()} ${url}`)
        await route.abort('blockedbyclient')
      }
    } catch {
      /* page/context closed mid-request */
    }
  })
  // Sockets bypass context.route; allow only the dev server (Vite HMR).
  await context.routeWebSocket('**', (ws) => {
    let origin = ''
    try {
      origin = new URL(ws.url()).origin.replace(/^ws/, 'http')
    } catch {
      /* block */
    }
    if (origin === allowedOrigin) ws.connectToServer()
    else {
      agent.noteBlocked(`WS ${ws.url()}`)
      void ws.close()
    }
  })
}

export const test = base.extend<{ agent: MockAgent; authenticated: boolean; lang: 'en' | 'zh' }>({
  /** Set `test.use({ authenticated: false })` to start at the login screen. */
  authenticated: [true, { option: true }],
  /** UI language. Specs assert English text; `test.use({ lang: 'zh' })` checks the Chinese UI. */
  lang: ['en', { option: true }],

  agent: [
    async ({ context, baseURL }, use, testInfo) => {
      const agent = new AgentImpl()
      registerDefaults(agent)
      await installRouting(context, agent, new URL(baseURL ?? 'http://127.0.0.1:5199').origin)
      await use(agent)
      agent.releaseAll()
      // Auto assertions. Skipped when the test already failed, to keep the first error visible.
      if (testInfo.status === 'passed' || testInfo.status === undefined) {
        expect(
          agent.unhandled().map((r) => `${r.method} ${r.path}`),
          'requests to the synthetic agent with no registered handler',
        ).toEqual([])
        expect(agent.blocked(), 'requests that left the dev server and agent origins').toEqual([])
      }
    },
    { auto: true },
  ],

  context: async ({ context, authenticated, lang }, use) => {
    // Unless the page itself switched language (Settings → Language), start in `lang`.
    await context.addInitScript((initial) => {
      try {
        if (!localStorage.getItem('u60_lang')) localStorage.setItem('u60_lang', initial)
      } catch {
        /* storage unavailable */
      }
    }, lang)
    if (authenticated) {
      // Seed once per tab so a logout/401 test is not re-authenticated by a reload.
      await context.addInitScript((token) => {
        try {
          if (!sessionStorage.getItem('__pw_seeded')) {
            sessionStorage.setItem('zte_token', token)
            sessionStorage.setItem('__pw_seeded', '1')
          }
        } catch {
          /* storage unavailable */
        }
      }, SYNTHETIC_TOKEN)
    }
    await use(context)
  },

  // Any test that uses `page` gets the agent (and its routing) installed first.
  page: async ({ page, agent }, use) => {
    void agent
    await use(page)
  },
})
