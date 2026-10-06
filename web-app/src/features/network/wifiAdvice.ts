// Configuration observations for one Wi-Fi band (PLAN2 R13).
//
// There is no neighbouring-network scan, so nothing here claims interference was measured or
// excluded; every line is a fact about the configured or the current radio state. An empty result
// means "nothing to say" and the UI omits the section.

import { t } from '../../i18n'
import type { WifiBand } from '../../types'
import { widthMhz, widthsAgree } from '../../data/wifiWidth'
import { normalizeConfiguredChannel } from './wifiDraft'

const DFS_5G_CHANNELS = new Set([52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 144])

export function getBandInsights(suffix: '2g' | '5g', band: WifiBand): string[] {
  const insights: string[] = []
  const configured = normalizeConfiguredChannel(band.configuredChannel)
  const current = band.actualChannel ?? band.channel

  if (configured === 'auto') {
    if (current != null) insights.push(t('Automatic channel selection is currently using channel {channel}.', { channel: current }))
  } else {
    const num = parseInt(configured, 10)
    if (!Number.isNaN(num)) {
      if (current != null && num !== current) {
        insights.push(t('Configured channel is {configured}; the radio is currently on channel {current}.', { configured: num, current }))
      }
      if (suffix === '2g' && ![1, 6, 11].includes(num)) {
        insights.push(
          t(
            'Channel {channel} overlaps its neighbouring 2.4 GHz channels; 1, 6 and 11 are the non-overlapping set. No scan of nearby networks was run, so interference is neither measured nor ruled out.',
            { channel: num },
          ),
        )
      }
      if (suffix === '5g' && DFS_5G_CHANNELS.has(num)) {
        insights.push(t('Channel {channel} is a DFS channel: radar detection can force the radio to change channel.', { channel: num }))
      }
    }
  }

  // Compare numeric widths so 'HE80' and '80 MHz' agree; unknown or unparseable widths never warn.
  if (widthsAgree(band.configuredBandwidth, band.actualBandwidth ?? band.bandwidth) === false) {
    insights.push(
      t('Configured width is {configured} MHz; the radio is currently operating at {current} MHz. The two can differ temporarily.', {
        configured: widthMhz(band.configuredBandwidth) ?? '',
        current: widthMhz(band.actualBandwidth ?? band.bandwidth) ?? '',
      }),
    )
  }
  return insights
}
