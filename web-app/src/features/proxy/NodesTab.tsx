import { useState } from 'react'
import { api } from '../../data/api'
import { usePoll } from '../../data/poll'
import type { ProxyGroups } from '../../types'
import { Button, Input, Segmented } from '../../ui/controls'
import { toast, toastError } from '../../ui/feedback'
import { Card, Chip, Empty, InlineStatus, Skeleton } from '../../ui/primitives'
import { delayLabel, delayTone, type NodeSort, visibleNodes } from './proxyView'

export default function NodesTab() {
  const groups = usePoll<ProxyGroups>('proxy-groups', api.proxyGroups, 15_000)
  const [query, setQuery] = useState('')
  const [sort, setSort] = useState<NodeSort>('name')
  const [testing, setTesting] = useState(false)
  const [selecting, setSelecting] = useState<string | null>(null)

  async function select(name: string) {
    if (selecting) return
    setSelecting(name)
    try {
      await api.proxySelect(name)
      toast(name === 'AUTO' ? 'Using the fastest node automatically' : `Using ${name}`)
      groups.refresh()
    } catch (e) {
      toastError(e, 'Failed to switch node')
    } finally {
      setSelecting(null)
    }
  }

  async function testAll() {
    setTesting(true)
    try {
      const delays = await api.proxyDelay()
      const ok = Object.values(delays).filter((d) => d > 0).length
      toast(`Latency tested: ${ok} node${ok === 1 ? '' : 's'} reachable`)
      setSort('delay')
    } catch (e) {
      toastError(e, 'Latency test failed')
    } finally {
      setTesting(false)
      groups.refresh()
    }
  }

  if (groups.status === 'loading') return <Skeleton className="h-48" />
  const g = groups.data
  if (!g) {
    return (
      <InlineStatus kind="error" action={{ label: 'Retry', onClick: groups.refresh, loading: groups.refreshing }}>
        Nodes could not be read{groups.error ? `: ${groups.error}` : '.'}
      </InlineStatus>
    )
  }
  if (!g.running) {
    return (
      <InlineStatus kind="info" live={false}>
        Start the proxy (Overview) to choose nodes.
      </InlineStatus>
    )
  }

  const proxy = g.groups.find((x) => x.name === 'PROXY')
  const auto = g.groups.find((x) => x.name === 'AUTO')
  const now = proxy?.now
  const multipleSubs = new Set(g.nodes.map((n) => n.subscription_id)).size > 1
  const nodes = visibleNodes(g.nodes, query, sort)

  return (
    <>
      <Card title="Route">
        <div className="space-y-3">
          <p className="text-body text-ink2">
            Proxied traffic leaves through{' '}
            <span className="font-semibold text-ink">
              {now === 'AUTO' && auto?.now ? `AUTO → ${auto.now}` : (now ?? '—')}
            </span>
            .
          </p>
          <div className="flex flex-wrap gap-2">
            {auto && (
              <Button
                variant={now === 'AUTO' ? 'primary' : 'outline'}
                onClick={() => void select('AUTO')}
                loading={selecting === 'AUTO'}
                disabled={!!selecting}
                aria-pressed={now === 'AUTO'}
              >
                Fastest (auto)
              </Button>
            )}
            <Button
              variant={now === 'DIRECT' ? 'primary' : 'outline'}
              onClick={() => void select('DIRECT')}
              loading={selecting === 'DIRECT'}
              disabled={!!selecting}
              aria-pressed={now === 'DIRECT'}
            >
              Direct
            </Button>
          </div>
        </div>
      </Card>

      <Card
        title={`Nodes${g.nodes.length ? ` (${g.nodes.length})` : ''}`}
        action={
          <Button size="sm" variant="outline" onClick={() => void testAll()} loading={testing} disabled={!g.nodes.length}>
            Test latency
          </Button>
        }
      >
        {g.nodes.length === 0 ? (
          <Empty title="No nodes yet" body="Add a subscription, or update it if it was added while the proxy was stopped." />
        ) : (
          <div className="space-y-3">
            <div className="flex flex-wrap items-center gap-2">
              <div className="min-w-40 flex-1">
                <Input
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  placeholder="Filter nodes (e.g. HK, Japan)"
                  aria-label="Filter nodes"
                />
              </div>
              <Segmented<NodeSort>
                label="Sort nodes"
                options={[
                  { value: 'name', label: 'Name' },
                  { value: 'delay', label: 'Latency' },
                ]}
                value={sort}
                onChange={setSort}
              />
            </div>
            {nodes.length === 0 ? (
              <p className="py-4 text-center text-meta text-ink3">No node matches “{query}”.</p>
            ) : (
              <ul className="max-h-[36rem] divide-y divide-line/6 overflow-y-auto">
                {nodes.map((n) => {
                  const active = now === n.name
                  return (
                    <li key={`${n.subscription_id}/${n.name}`}>
                      <button
                        type="button"
                        onClick={() => void select(n.name)}
                        disabled={!!selecting}
                        aria-pressed={active}
                        className={`flex w-full items-center justify-between gap-3 px-1 py-2 text-left transition-colors hover:bg-surface2 coarse:min-h-11 disabled:opacity-60 ${
                          active ? 'text-accent' : 'text-ink'
                        }`}
                      >
                        <span className="flex min-w-0 items-center gap-2">
                          <span
                            className={`h-2 w-2 shrink-0 rounded-full ${active ? 'bg-accent' : 'bg-transparent'}`}
                            aria-hidden="true"
                          />
                          <span className={`truncate text-body ${active ? 'font-semibold' : 'font-medium'}`}>{n.name}</span>
                        </span>
                        <span className="flex shrink-0 items-center gap-1.5">
                          {multipleSubs && <Chip>{n.subscription}</Chip>}
                          {n.type && <Chip>{n.type}</Chip>}
                          <Chip tone={delayTone(n.delay)}>{selecting === n.name ? '…' : delayLabel(n.delay)}</Chip>
                        </span>
                      </button>
                    </li>
                  )
                })}
              </ul>
            )}
          </div>
        )}
      </Card>
    </>
  )
}
