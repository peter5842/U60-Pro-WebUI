import { app } from './app'
import { common } from './common'
import { home } from './home'
import { modem } from './modem'
import { network } from './network'
import { proxy } from './proxy'
import { signal } from './signal'
import { system } from './system'

/** All Chinese strings. Areas are split so translations can be edited independently. */
export const ZH: Record<string, string> = { ...common, ...app, ...home, ...signal, ...network, ...modem, ...proxy, ...system }

/** Per-area catalogs, for the duplicate check in tools/test-i18n.cjs. */
export const AREAS: Record<string, Record<string, string>> = { common, app, home, signal, network, modem, proxy, system }
