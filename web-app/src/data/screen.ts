// Mapper for the geek screen (agent/src/screen.rs).

import type { ScreenAutoOpen, ScreenPage, ScreenSettings, ScreenStatus, ScreenTheme } from '../types'
import { boolLike, intInRange, nonEmptyStr } from './validate'

const STATES: ScreenStatus['state'][] = ['idle', 'starting', 'active', 'restoring', 'refused', 'error', 'stale']
export const SCREEN_PAGES: ScreenPage[] = ['home', 'sys', 'pwr', 'cell', 'net', 'lan', 'proxy']

const oneOf = <T extends string>(v: unknown, allowed: readonly T[], fallback: T): T =>
  typeof v === 'string' && (allowed as readonly string[]).includes(v) ? (v as T) : fallback

export function mapScreenSettings(d: Record<string, unknown>): ScreenSettings {
  return {
    brightness: intInRange(d.brightness, 20, 255) ?? 160,
    idle_secs: intInRange(d.idle_secs, 30, 3600) ?? 120,
    idle_action: oneOf(d.idle_action, ['stock', 'sleep'] as const, 'stock'),
    theme: oneOf<ScreenTheme>(d.theme, ['green', 'amber', 'cyan'], 'green'),
    scanlines: boolLike(d.scanlines) ?? true,
    start_page: oneOf(d.start_page, SCREEN_PAGES, 'home'),
    gesture: boolLike(d.gesture) ?? true,
    auto_open: oneOf<ScreenAutoOpen>(d.auto_open, ['off', 'boot', 'wake'], 'off'),
  }
}

export function mapScreen(d: Record<string, unknown>): ScreenStatus {
  const session = d.session && typeof d.session === 'object' ? (d.session as Record<string, unknown>) : {}
  const settings = d.settings && typeof d.settings === 'object' ? (d.settings as Record<string, unknown>) : {}
  return {
    installed: boolLike(d.installed) ?? false,
    state: oneOf(session.state, STATES, 'unknown'),
    reason: nonEmptyStr(session.reason),
    version: nonEmptyStr(session.version),
    gesture_listener: boolLike(d.gesture_listener) ?? false,
    settings: mapScreenSettings(settings),
  }
}
