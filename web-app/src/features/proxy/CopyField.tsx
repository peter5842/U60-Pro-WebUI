import { useId } from 'react'
import { Button, Field, Input } from '../../ui/controls'
import { toast } from '../../ui/feedback'

/**
 * Read-only value with a Copy button. The dashboard is served over plain HTTP,
 * where `navigator.clipboard` is unavailable, so fall back to selecting the
 * text and `execCommand('copy')`; if that fails too the text stays selected.
 */
export default function CopyField({ label, value, hint }: { label: string; value: string; hint?: string }) {
  const inputId = useId()

  async function copy() {
    try {
      if (navigator.clipboard && window.isSecureContext) {
        await navigator.clipboard.writeText(value)
        toast(`${label} copied`)
        return
      }
    } catch {
      // fall through to the selection fallback
    }
    const el = document.getElementById(inputId)
    if (!(el instanceof HTMLInputElement)) return
    el.focus()
    el.select()
    const copied = document.execCommand?.('copy')
    toast(copied ? `${label} copied` : 'Selected — press Ctrl/⌘+C to copy', copied ? 'ok' : 'err')
  }

  return (
    <Field label={label} hint={hint} id={inputId}>
      {(ids) => (
        <div className="flex gap-2">
          <Input
            id={ids.id}
            readOnly
            value={value}
            onFocus={(e) => e.currentTarget.select()}
            className="font-mono text-meta"
            aria-describedby={ids.describedBy}
          />
          <Button variant="outline" onClick={() => void copy()}>
            Copy
          </Button>
        </div>
      )}
    </Field>
  )
}
