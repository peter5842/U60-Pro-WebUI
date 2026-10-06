import { useMemo, useState } from 'react'
import { api } from '../../data/api'
import { usePoll } from '../../data/poll'
import { IChevronDown } from '../../icons'
import { t } from '../../i18n'
import type { ProxyGroup, ProxyGroups, ProxyNode } from '../../types'
import { Button, Input, Segmented } from '../../ui/controls'
import { toast, toastError } from '../../ui/feedback'
import { Card, Chip, Empty, InlineStatus, Skeleton } from '../../ui/primitives'
import { delayLabel, delayTone, groupTypeLabel, isSelectable, type NodeSort, visibleMembers } from './proxyView'

export default function NodesTab() {
  const groups = usePoll<ProxyGroups>('proxy-groups', api.proxyGroups, 15_000)
  const [query, setQuery] = useState('')
  const [sort, setSort] = useState<NodeSort>('config')
  const [testingAll, setTestingAll] = useState(false)
  // Groups the user opened or closed; the first group starts open.
  const [toggled, setToggled] = useState<Record<string, boolean>>({})

  const data = groups.data
  const nodes = useMemo(() => new Map((data?.nodes ?? []).map((n) => [n.name, n])), [data])
  const byName = useMemo(() => new Map((data?.groups ?? []).map((g) => [g.name, g])), [data])

  async function testAll() {
    setTestingAll(true)
    try {
      await api.proxyDelay()
      toast(t('Latency tested'))
      setSort('delay')
    } catch (e) {
      toastError(e, t('Latency test failed'))
    } finally {
      setTestingAll(false)
      groups.refresh()
    }
  }

  if (groups.status === 'loading') return <Skeleton className="h-48" />
  if (!data) {
    return (
      <InlineStatus kind="error" action={{ label: t('Retry'), onClick: groups.refresh, loading: groups.refreshing }}>
        {groups.error ? t('Nodes could not be read: {error}', { error: groups.error }) : t('Nodes could not be read.')}
      </InlineStatus>
    )
  }
  if (!data.running) {
    return (
      <InlineStatus kind="info" live={false}>
        {t('Start the proxy (Overview) to choose nodes.')}
      </InlineStatus>
    )
  }

  const visible = data.groups.filter((g) => !g.hidden)
  const multipleSubs = new Set(data.nodes.map((n) => n.subscription_id).filter(Boolean)).size > 1

  return (
    <>
      <Card
        title={t('Nodes ({n})', { n: data.nodes.filter(isRealNode).length })}
        action={
          <Button size="sm" variant="outline" onClick={() => void testAll()} loading={testingAll}>
            {t('Test all')}
          </Button>
        }
      >
        <div className="flex flex-wrap items-center gap-2">
          <div className="min-w-40 flex-1">
            <Input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t('Filter nodes (e.g. HK, Japan)')}
              aria-label={t('Filter nodes')}
            />
          </div>
          <Segmented<NodeSort>
            label={t('Sort nodes')}
            options={[
              { value: 'config', label: t('Default order') },
              { value: 'delay', label: t('Latency') },
            ]}
            value={sort}
            onChange={setSort}
          />
        </div>
      </Card>

      {visible.length === 0 ? (
        <Empty title={t('No nodes yet')} body={t('Add a subscription, or update it if it was added while the proxy was stopped.')} />
      ) : (
        visible.map((group, i) => (
          <GroupCard
            key={group.name}
            group={group}
            open={toggled[group.name] ?? i === 0}
            onToggle={() => setToggled((prev) => ({ ...prev, [group.name]: !(prev[group.name] ?? i === 0) }))}
            nodes={nodes}
            groups={byName}
            query={query}
            sort={sort}
            showSubscription={multipleSubs}
            onChanged={groups.refresh}
          />
        ))
      )}
    </>
  )
}

/** A proxy that is not a built-in policy (DIRECT, REJECT…). */
function isRealNode(n: ProxyNode): boolean {
  return !['Direct', 'Reject', 'RejectDrop', 'Pass', 'Compatible'].includes(n.type ?? '')
}

function GroupCard({
  group,
  open,
  onToggle,
  nodes,
  groups,
  query,
  sort,
  showSubscription,
  onChanged,
}: {
  group: ProxyGroup
  open: boolean
  onToggle: () => void
  nodes: Map<string, ProxyNode>
  groups: Map<string, ProxyGroup>
  query: string
  sort: NodeSort
  showSubscription: boolean
  onChanged: () => void
}) {
  const [busy, setBusy] = useState<string | null>(null)
  const selectable = isSelectable(group)
  const members = visibleMembers(group.all, nodes, query, sort)
  // With a filter, show matching groups open and hide groups with no match.
  const filtering = query.trim() !== ''
  if (filtering && members.length === 0) return null
  const expanded = open || filtering

  async function select(name: string) {
    if (busy || name === group.now) return
    setBusy(name)
    try {
      await api.proxySelect(group.name, name)
      toast(t('{group}: using {name}', { group: group.name, name }))
      onChanged()
    } catch (e) {
      toastError(e, t('Failed to switch node'))
    } finally {
      setBusy(null)
    }
  }

  async function test() {
    setBusy('__test')
    try {
      const delays = await api.proxyDelay(group.name)
      const ok = Object.values(delays).filter((d) => d > 0).length
      toast(t('{group}: {n} reachable', { group: group.name, n: ok }))
    } catch (e) {
      toastError(e, t('Latency test failed'))
    } finally {
      setBusy(null)
      onChanged()
    }
  }

  const nowGroup = group.now ? groups.get(group.now) : undefined
  const nowLabel = nowGroup?.now ? `${group.now} → ${nowGroup.now}` : (group.now ?? '—')

  return (
    <section className="rounded-panel border border-line/8 bg-surface">
      <header className="flex items-center justify-between gap-2 px-4 py-2.5">
        <button
          type="button"
          onClick={onToggle}
          aria-expanded={expanded}
          className="flex min-w-0 flex-1 items-center gap-2 text-left coarse:min-h-11"
        >
          <IChevronDown size={15} className={`shrink-0 text-ink3 transition-transform ${expanded ? '' : '-rotate-90'}`} />
          <span className="font-display text-sm font-semibold text-ink">{group.name}</span>
          <Chip>{groupTypeLabel(group.type)}</Chip>
          <span className="min-w-0 truncate text-meta text-ink2">{nowLabel}</span>
        </button>
        <Button size="sm" variant="ghost" onClick={() => void test()} loading={busy === '__test'} disabled={!!busy}>
          {t('Test')}
        </Button>
      </header>
      {expanded && (
        <ul className="max-h-[28rem] divide-y divide-line/6 overflow-y-auto border-t border-line/8 px-3">
          {members.map((name) => {
            const node = nodes.get(name)
            const sub = groups.get(name)
            const active = group.now === name
            return (
              <li key={name}>
                <button
                  type="button"
                  onClick={() => void select(name)}
                  disabled={!selectable || !!busy}
                  aria-pressed={selectable ? active : undefined}
                  className={`flex w-full items-center justify-between gap-3 px-1 py-2 text-left transition-colors coarse:min-h-11 ${
                    selectable ? 'hover:bg-surface2' : 'cursor-default'
                  } ${active ? 'text-accent' : 'text-ink'} disabled:opacity-100`}
                >
                  <span className="flex min-w-0 items-center gap-2">
                    <span className={`h-2 w-2 shrink-0 rounded-full ${active ? 'bg-accent' : 'bg-transparent'}`} aria-hidden="true" />
                    <span className={`truncate text-body ${active ? 'font-semibold' : 'font-medium'}`}>{name}</span>
                  </span>
                  <span className="flex shrink-0 items-center gap-1.5">
                    {sub ? (
                      <>
                        <Chip tone="accent">{t('Group')}</Chip>
                        {sub.now && <span className="max-w-40 truncate text-meta text-ink3">→ {sub.now}</span>}
                      </>
                    ) : (
                      <>
                        {showSubscription && node?.subscription && <Chip>{node.subscription}</Chip>}
                        {node?.type && <Chip>{node.type}</Chip>}
                        {node && isRealNode(node) && (
                          <Chip tone={delayTone(node.delay)}>{busy === name ? '…' : delayLabel(node.delay)}</Chip>
                        )}
                      </>
                    )}
                  </span>
                </button>
              </li>
            )
          })}
        </ul>
      )}
    </section>
  )
}
