import { useState } from 'react'
import { Button, Field, Input } from '../../../ui/controls'
import { t } from '../../../i18n'
import { toast } from '../../../ui/feedback'
import { Card } from '../../../ui/primitives'
import type { CellTuple, Rat } from './confirmations'
import type { Ops } from './ops'

/** Manual cell lock by PCI and ARFCN. Validation is unchanged; the shared flow confirms and submits. */
export function CellLock({ type, ops, onLock }: { type: Rat; ops: Ops; onLock: (cell: CellTuple, successText: string) => Promise<void> }) {
  const [pci, setPci] = useState('')
  const [earfcn, setEarfcn] = useState('')
  const [band, setBand] = useState('')

  async function apply() {
    if (!pci || !earfcn) {
      toast(t('PCI and ARFCN are required'), 'err')
      return
    }
    if (type === 'nr' && !band) {
      toast(t('Band is required for NR cell lock'), 'err')
      return
    }
    const cell: CellTuple = { tech: type, pci, earfcn, band: type === 'nr' ? band : undefined } // frozen copy
    await onLock(cell, t('{tech} cell locked (PCI {pci})', { tech: type === 'nr' ? 'NR' : 'LTE', pci }))
  }

  return (
    <Card title={t('{tech} cell lock', { tech: type === 'nr' ? 'NR' : 'LTE' })}>
      <p className="mb-3 text-meta text-ink2">
        {t('Lock to a specific cell by PCI and {arfcn}.', { arfcn: type === 'nr' ? 'NR-ARFCN' : 'EARFCN' })}
      </p>
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
        <Field label="PCI">
          <Input type="number" inputMode="numeric" value={pci} onChange={(e) => setPci(e.target.value)} placeholder="30" />
        </Field>
        <Field label={type === 'nr' ? 'NR-ARFCN' : 'EARFCN'}>
          <Input type="number" inputMode="numeric" value={earfcn} onChange={(e) => setEarfcn(e.target.value)} placeholder={type === 'nr' ? '630912' : '3650'} />
        </Field>
        {type === 'nr' && (
          <Field label={t('Band')}>
            <Input type="number" inputMode="numeric" value={band} onChange={(e) => setBand(e.target.value)} placeholder="78" />
          </Field>
        )}
        <div className="flex items-end">
          <Button variant="primary" onClick={apply} disabled={ops.busy} className="w-full">
            {t('Lock')}
          </Button>
        </div>
      </div>
    </Card>
  )
}
