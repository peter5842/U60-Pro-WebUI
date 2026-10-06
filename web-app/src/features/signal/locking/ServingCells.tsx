import type { ReactNode } from 'react'
import { t } from '../../../i18n'
import type { CarrierComponent, SignalInfo } from '../../../types'
import { Button } from '../../../ui/controls'
import { Card, Chip, Unavailable } from '../../../ui/primitives'
import { ratName, type CellTuple, type Rat } from './confirmations'
import type { Ops } from './ops'

function lockName(c: CellTuple): string {
  const cell = { rat: ratName(c.tech), pci: c.pci, arfcnName: c.tech === 'nr' ? 'NR-ARFCN' : 'EARFCN', arfcn: c.earfcn }
  return c.tech === 'nr' && c.band
    ? t('Lock {rat} cell PCI {pci}, {arfcnName} {arfcn}, band n{band}', { ...cell, band: c.band })
    : t('Lock {rat} cell PCI {pci}, {arfcnName} {arfcn}', cell)
}

const num = (v: number | undefined) => (v === undefined ? <Unavailable /> : v)

interface Row {
  key: string
  tech: Rat
  carrier: CarrierComponent
  /** Present only when the firmware reported a valid PCI (0 is valid). */
  tuple: CellTuple | null
}

function toRows(carriers: CarrierComponent[], tech: Rat): Row[] {
  return carriers.map((carrier, i) => ({
    key: `${tech}-${i}`,
    tech,
    carrier,
    tuple:
      carrier.pci === undefined
        ? null
        : { tech, pci: String(carrier.pci), earfcn: String(carrier.earfcn), band: tech === 'nr' ? carrier.band.replace(/\D/g, '') : undefined },
  }))
}

/**
 * Serving cells. Desktop: a table. Below `sm`: stacked rows carrying the same fields. Each Lock
 * button names its own PCI/ARFCN/band and still goes through the shared confirmation.
 */
export function ServingCells({ signal, ops, onLock }: { signal: SignalInfo; ops: Ops; onLock: (cell: CellTuple) => void }) {
  const rows = [...toRows(signal.nr_carriers, 'nr'), ...toRows(signal.lte_carriers, 'lte')]
  if (rows.length === 0) return null

  const lockButton = (r: Row) => (
    <Button
      size="sm"
      variant="outline"
      disabled={r.tuple === null || ops.busy}
      aria-label={
        r.tuple
          ? lockName(r.tuple)
          : t('Lock unavailable for {tech} {band}: no valid PCI reported', { tech: r.tech === 'nr' ? 'NR' : 'LTE', band: r.carrier.band })
      }
      onClick={() => r.tuple && onLock(r.tuple)}
    >
      {t('Lock')}
    </Button>
  )
  const bandCls = (rat: Rat) => `font-semibold ${rat === 'nr' ? 'text-nr' : 'text-accent'}`
  const chipTone = (r: Row) => (r.carrier.label === 'PCC' ? (r.tech === 'nr' ? 'nr' : 'lte') : 'default')

  return (
    <Card title={t('Serving cells')}>
      <p className="mb-3 text-meta text-ink2">{t('Active cells. Locking asks for confirmation first.')}</p>

      <ul aria-label={t('Serving cells')} className="divide-y divide-line/6 sm:hidden">
        {rows.map((r) => (
          <li key={r.key} className="py-3 first:pt-0 last:pb-0">
            <div className="flex flex-wrap items-center gap-x-2 gap-y-1.5">
              <Chip tone={chipTone(r)}>{r.carrier.label}</Chip>
              <span className={`tnum font-mono ${bandCls(r.tech)}`}>{r.carrier.band}</span>
              <span className="ml-auto">{lockButton(r)}</span>
            </div>
            <dl className="tnum mt-2 grid grid-cols-3 gap-x-3 gap-y-2 font-mono text-meta">
              <Field term="PCI">{num(r.carrier.pci)}</Field>
              <Field term={r.tech === 'nr' ? 'NR-ARFCN' : 'EARFCN'}>{r.carrier.earfcn}</Field>
              <Field term={t('BW')}>{r.carrier.bandwidth || <Unavailable />}</Field>
              <Field term="RSRP">{num(r.carrier.rsrp)}</Field>
              <Field term="SINR">{num(r.carrier.sinr)}</Field>
            </dl>
          </li>
        ))}
      </ul>

      <div className="hidden overflow-x-auto sm:block">
        <table className="w-full text-left text-body">
          <thead>
            <tr className="label border-b border-line/8">
              <th className="pb-1.5 pr-3 font-semibold">{t('Type')}</th>
              <th className="pb-1.5 pr-3 font-semibold">{t('Band')}</th>
              <th className="pb-1.5 pr-3 font-semibold">PCI</th>
              <th className="pb-1.5 pr-3 font-semibold">ARFCN</th>
              <th className="pb-1.5 pr-3 font-semibold">{t('BW')}</th>
              <th className="pb-1.5 pr-3 font-semibold">RSRP</th>
              <th className="pb-1.5 pr-3 font-semibold">SINR</th>
              <th className="pb-1.5">
                <span className="sr-only">{t('Action')}</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.key} className="border-b border-line/6 last:border-0">
                <td className="py-1.5 pr-3">
                  <Chip tone={chipTone(r)}>{r.carrier.label}</Chip>
                </td>
                <td className={`py-1.5 pr-3 ${bandCls(r.tech)}`}>{r.carrier.band}</td>
                <td className="tnum font-mono py-1.5 pr-3">{num(r.carrier.pci)}</td>
                <td className="tnum font-mono py-1.5 pr-3">{r.carrier.earfcn}</td>
                <td className="tnum font-mono py-1.5 pr-3 text-ink2">{r.carrier.bandwidth || <Unavailable />}</td>
                <td className="tnum font-mono py-1.5 pr-3">{num(r.carrier.rsrp)}</td>
                <td className="tnum font-mono py-1.5 pr-3">{num(r.carrier.sinr)}</td>
                <td className="py-1.5 text-right">{lockButton(r)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Card>
  )
}

function Field({ term, children }: { term: string; children: ReactNode }) {
  return (
    <div className="min-w-0">
      <dt className="label">{term}</dt>
      <dd className="break-words text-ink">{children}</dd>
    </div>
  )
}
