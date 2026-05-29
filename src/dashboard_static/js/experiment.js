// ─── Experiment tab (intuitive UX) ───────────────────────
const EXP_FEATURE_LABELS = {
    profile: 'Profile filtering',
    inject: 'System prefix',
    adaptive: 'Adaptive prefix',
    coaching: 'Coaching',
};
/** One primary success metric per feature A/B test. */
const EXP_FEATURE_KPI = {
    profile: {
        question: 'Does stripping tool schemas save tokens without getting in your way?',
        success: 'With filtering on, ctx strips tools every turn, input tokens drop, and whole-turn cost stays flat or falls once you have about 100 prompts per side.',
        primaryName: 'Tool savings',
        ctxSection: 'What ctx controls',
        billSection: 'Whole-turn bill check',
    },
    inject: {
        question: 'Does your static prefix help Claude get it right with fewer redirect turns?',
        success: 'With the prefix on, correction rate falls or cost per turn drops. Extra input tokens are normal. You trade a little context for fewer back-and-forths.',
        primaryName: 'Quality, then cost',
        ctxSection: 'Prefix footprint',
        billSection: 'Does the quality gain pay off?',
    },
    adaptive: {
        question: 'Does context from similar past sessions cut wasted turns?',
        success: 'With adaptive on, you see fewer corrections and the same or lower cost per turn. The text comes from your history, not a generic blurb.',
        primaryName: 'Quality, then cost',
        ctxSection: 'Adaptive context added',
        billSection: 'Does the quality gain pay off?',
    },
    coaching: {
        question: 'Do coaching nudges stop correction spirals before they burn budget?',
        success: 'With coaching on, session correction rate drops. Cost usually follows once the sample is big enough. Quality leads, dollars lag.',
        primaryName: 'Correction rate',
        ctxSection: 'Coaching activity',
        billSection: 'Cost follow-up',
    },
};
const EXP_EMPTY = UI_EMPTY;
const EXP_AB_CODE = {
    P: 'profile',
    I: 'inject',
    A: 'adaptive',
    C: 'coaching'
};
const EXP_CTX_SAVINGS_RATE = 0.30;
let _abDailyRows = [];
let _abTrendChart = null;
let _expAbActive = false;

function expPerPromptSavings(withCost, withoutCost) {
    return withoutCost - withCost;
}

function expFmtCents(n) {
    if (n == null || n <= 0) return EXP_EMPTY;
    if (n < 0.01) return '<1¢';
    return Math.round(n * 100) + '¢';
}

function expCtxSavingsUsd(avgTokensSaved) {
    return (avgTokensSaved / 1_000_000) * EXP_CTX_SAVINGS_RATE;
}

function expFmtTokShort(n) {
    if (!n || n <= 0) return '0';
    return n >= 1000 ? (n / 1000).toFixed(1) + 'K' : String(Math.round(n));
}

function expIsEarlySample(withCount, withoutCount) {
    return withCount < 100 || withoutCount < 100;
}

function expProfileDirectSavingsLine(t) {
    const tok = t.avg_tokens_saved || 0;
    const tools = t.avg_tools_removed || 0;
    if (tok <= 0 && tools <= 0) return null;
    const usd = expCtxSavingsUsd(tok);
    const toolPart = tools > 0 ? `-${Math.round(tools)} tools, ` : '';
    return toolPart + expFmtTokShort(tok) + ' tokens stripped, ~' + expFmtCents(usd) + ' ctx savings per turn';
}

function expFmtTokCell(n) {
    if (!n || n <= 0) return '0';
    return Math.round(n).toLocaleString();
}

function expFmtCacheCell(n) {
    if (!n || n <= 0) return '0';
    return expFmtTokShort(n);
}

function expCompareNote(offVal, onVal, opts) {
    const {
        lowerIsBetter,
        sameThresholdPct,
        onLabel,
        offLabel
    } = opts;
    if (!offVal && !onVal) return '';
    if (!offVal || !onVal) return '';
    const diff = onVal - offVal;
    const base = Math.max(Math.abs(offVal), Math.abs(onVal), 1);
    if (Math.abs(diff) / base <= (sameThresholdPct || 0.08)) {
        return 'about the same';
    }
    if (lowerIsBetter) {
        if (onVal < offVal) return 'lower with feature on';
        if (onVal > offVal) return 'higher with feature on';
    } else {
        if (onVal > offVal) return 'higher with feature on';
        if (onVal < offVal) return 'lower with feature on';
    }
    return '';
}

function expLearningInsight(f, t, c) {
    const feature = f.feature;
    if (feature === 'profile') {
        const inputDown = t.avg_input_tokens > 0 && t.avg_input_tokens < c.avg_input_tokens * 0.85;
        const costUp = t.avg_cost_usd > c.avg_cost_usd * 1.05;
        const cacheUp = t.avg_cache_read_tokens > c.avg_cache_read_tokens * 1.08;
        const parts = [];
        if (inputDown) {
            parts.push('Filtering is cutting input tokens because tool schemas are removed before the API call.');
        }
        if (t.avg_tools_removed > 0) {
            parts.push('Ctx saves about ' + expFmtCents(expCtxSavingsUsd(t.avg_tokens_saved)) + ' per turn from stripped tools.');
        }
        if (costUp && inputDown) {
            parts.push('Whole-turn cost is higher with filtering on so far. That is output and cache, not "more tools means more cost".');
        } else if (cacheUp && costUp) {
            parts.push('Cache read is heavier on feature-on prompts. That can move the total bill more than tool savings.');
        }
        if (!parts.length) {
            return 'Compare arms side by side below. Whole-turn cost needs ~100 prompts per arm before it is actionable.';
        }
        return parts.join(' ');
    }
    if (feature === 'inject' || feature === 'adaptive') {
        const inputDiff = t.avg_input_tokens - c.avg_input_tokens;
        const costSaved = expCostPerTurnDelta(t, c);
        const corrDown = t.correction_rate_pct < c.correction_rate_pct - 2;
        const parts = [];
        if (corrDown) {
            parts.push('Correction rate is lower with the prefix on. That is the quality win this test tracks.');
        }
        if (costSaved > 0.005) {
            parts.push('Whole-turn cost is also lower on feature-on prompts so far.');
        } else if (Math.abs(inputDiff) > 100 && !corrDown) {
            parts.push('The prefix adds input tokens each turn. That alone is not failure. Watch correction rate and cost per turn.');
        }
        if (!parts.length) {
            return 'We are looking for correction rate down and/or cost per turn down vs the off arm. Need about 100 prompts per side.';
        }
        return parts.join(' ');
    }
    if (feature === 'coaching') {
        const corrDown = t.correction_rate_pct < c.correction_rate_pct - 2;
        if (corrDown) {
            return 'Coaching lines up with fewer corrections in this sample. That is the leading success signal.';
        }
        if (t.coach_fire_rate_pct > 0) {
            return `Coaching fired on ${t.coach_fire_rate_pct.toFixed(0)}% of on-arm turns. Compare correction rate once both sides have about 100 prompts.`;
        }
        return 'Coaching only fires when ctx spots redirect patterns. We measure correction rate, not prefix size.';
    }
    const saved = expPerPromptSavings(t.avg_cost_usd, c.avg_cost_usd);
    if (Math.abs(saved) < 0.005) return 'Whole-turn cost looks similar so far. Keep collecting prompts.';
    if (saved > 0) return 'Feature-on prompts are cheaper on average so far. Still early; confirm at about 100 prompts per side.';
    return 'Feature-on prompts cost more on average so far. Check the input and output breakdown before turning off.';
}

function expCompareTr(label, off, on, note) {
    return `<tr>
      <td>${esc(label)}</td>
      <td class="num">${off}</td>
      <td class="num">${on}</td>
      <td class="exp-note">${note ? esc(note) : ""}</td>
    </tr>`;
}

function expCompareTable(title, rows, offN, onN) {
    if (!rows.length) return "";
    return `<div class="exp-table-block">
      <div class="exp-table-title">${esc(title)}</div>
      <table class="data-table exp-table">
        <thead>
          <tr>
            <th>Metric</th>
            <th class="num">Off (${offN})</th>
            <th class="num">On (${onN})</th>
            <th>Note</th>
          </tr>
        </thead>
        <tbody>${rows.join("")}</tbody>
      </table>
    </div>`;
}

function expKpiBanner(feature) {
    const k = EXP_FEATURE_KPI[feature] || EXP_FEATURE_KPI.profile;
    return `<div class="exp-kpi-banner">
      <p class="exp-kpi-question">${esc(k.question)}</p>
      <p class="exp-kpi-success"><strong>Winning looks like:</strong> ${esc(k.success)}</p>
    </div>`;
}

function expCostPerTurnDelta(t, c) {
    return expPerPromptSavings(t.avg_cost_usd, c.avg_cost_usd);
}

function expHeroPrimary(feature, t, c) {
    const kpi = EXP_FEATURE_KPI[feature] || EXP_FEATURE_KPI.profile;
    if (feature === 'profile' && (t.avg_tokens_saved > 0 || t.avg_tools_removed > 0)) {
        const tools = t.avg_tools_removed > 0 ? Math.round(t.avg_tools_removed) : 0;
        const tok = expFmtTokShort(t.avg_tokens_saved || 0);
        const usd = expFmtCents(expCtxSavingsUsd(t.avg_tokens_saved));
        return {
            title: kpi.primaryName,
            value: tools > 0 ? `-${tools} tools, ${tok} tok` : `${tok} tok stripped`,
            sub: `About ${usd} per turn in ctx savings. Compare whole-turn cost in the bill check below.`,
            tone: 'positive',
        };
    }
    if (feature === 'inject' || feature === 'adaptive') {
        const costDelta = expCostPerTurnDelta(t, c);
        const corrDelta = t.correction_rate_pct - c.correction_rate_pct;
        const prefixChars = Math.round(feature === 'inject' ? (t.avg_inject_chars || 0) : (t.avg_adaptive_chars || 0));
        if (costDelta > 0.005) {
            return {
                title: kpi.primaryName,
                value: `${expFmtCents(costDelta)} less per turn`,
                sub: `Correction rate ${t.correction_rate_pct.toFixed(0)}% on vs ${c.correction_rate_pct.toFixed(0)}% off. Prefix adds about ${prefixChars} chars when on.`,
                tone: 'positive',
            };
        }
        if (corrDelta <= -3 && c.correction_rate_pct > 0) {
            return {
                title: kpi.primaryName,
                value: `${Math.abs(corrDelta).toFixed(0)}pt fewer corrections`,
                sub: `On ${t.correction_rate_pct.toFixed(0)}% vs off ${c.correction_rate_pct.toFixed(0)}%. Watch cost per turn as the sample grows.`,
                tone: 'positive',
            };
        }
        return {
            title: kpi.primaryName,
            value: prefixChars > 0 ? `+${prefixChars} chars/turn when on` : 'Collecting',
            sub: 'We want correction rate down and/or cost per turn down. Input tokens alone do not decide this.',
            tone: 'neutral',
        };
    }
    if (feature === 'coaching') {
        const corrDelta = t.correction_rate_pct - c.correction_rate_pct;
        const fires = t.coach_fire_rate_pct || 0;
        if (corrDelta <= -3 && c.correction_rate_pct > 0) {
            return {
                title: kpi.primaryName,
                value: `${Math.abs(corrDelta).toFixed(0)}pt fewer corrections`,
                sub: `On ${t.correction_rate_pct.toFixed(0)}% vs off ${c.correction_rate_pct.toFixed(0)}%. Nudges fired on ${fires.toFixed(0)}% of on-arm turns.`,
                tone: 'positive',
            };
        }
        return {
            title: kpi.primaryName,
            value: `${t.correction_rate_pct.toFixed(0)}% corrections`,
            sub: `Off arm ${c.correction_rate_pct.toFixed(0)}%. Nudges fire on ${fires.toFixed(0)}% of on-arm turns when patterns show up.`,
            tone: 'neutral',
        };
    }
    return {
        title: kpi.primaryName,
        value: EXP_EMPTY,
        sub: 'Need more prompts per arm',
        tone: 'muted',
    };
}

function expComparisonPanel(f, t, c) {
    const feature = f.feature;
    const kpi = EXP_FEATURE_KPI[feature] || EXP_FEATURE_KPI.profile;
    const ctxRows = [];
    if (feature === "profile") {
        ctxRows.push(expCompareTr("Tools stripped / turn", "0", t.avg_tools_removed > 0 ? "~" + Math.round(t.avg_tools_removed) : "0", t.avg_tools_removed > 0 ? "main metric" : ""));
        ctxRows.push(expCompareTr("Est. ctx savings / turn", "$0", t.avg_tokens_saved > 0 ? "~" + expFmtCents(expCtxSavingsUsd(t.avg_tokens_saved)) : "$0", t.avg_tokens_saved > 0 ? "tool schemas only" : ""));
    } else if (feature === "inject") {
        ctxRows.push(expCompareTr("Prefix chars / turn", c.avg_inject_chars > 0 ? "~" + Math.round(c.avg_inject_chars) : "0", t.avg_inject_chars > 0 ? "~" + Math.round(t.avg_inject_chars) : "0", "expected cost, not the verdict"));
        ctxRows.push(expCompareTr("Correction rate", c.correction_rate_pct.toFixed(0) + "%", t.correction_rate_pct.toFixed(0) + "%", expCompareNote(c.correction_rate_pct, t.correction_rate_pct, { lowerIsBetter: true, sameThresholdPct: 0.15 })));
        ctxRows.push(expCompareTr("Avg cost / turn", fmtCost(c.avg_cost_usd), fmtCost(t.avg_cost_usd), expCompareNote(c.avg_cost_usd, t.avg_cost_usd, { lowerIsBetter: true, sameThresholdPct: 0.05 })));
    } else if (feature === "adaptive") {
        ctxRows.push(expCompareTr("Adaptive chars / turn", c.avg_adaptive_chars > 0 ? "~" + Math.round(c.avg_adaptive_chars) : "0", t.avg_adaptive_chars > 0 ? "~" + Math.round(t.avg_adaptive_chars) : "0", "context added when on"));
        ctxRows.push(expCompareTr("Correction rate", c.correction_rate_pct.toFixed(0) + "%", t.correction_rate_pct.toFixed(0) + "%", expCompareNote(c.correction_rate_pct, t.correction_rate_pct, { lowerIsBetter: true, sameThresholdPct: 0.15 })));
        ctxRows.push(expCompareTr("Avg cost / turn", fmtCost(c.avg_cost_usd), fmtCost(t.avg_cost_usd), expCompareNote(c.avg_cost_usd, t.avg_cost_usd, { lowerIsBetter: true, sameThresholdPct: 0.05 })));
    } else if (feature === "coaching") {
        ctxRows.push(expCompareTr("Correction rate", c.correction_rate_pct.toFixed(0) + "%", t.correction_rate_pct.toFixed(0) + "%", expCompareNote(c.correction_rate_pct, t.correction_rate_pct, { lowerIsBetter: true, sameThresholdPct: 0.15 })));
        ctxRows.push(expCompareTr("Coaching nudges / turn", (c.coach_fire_rate_pct || 0).toFixed(0) + "%", (t.coach_fire_rate_pct || 0).toFixed(0) + "%", "fires when redirect patterns detected"));
        ctxRows.push(expCompareTr("Avg cost / turn", fmtCost(c.avg_cost_usd), fmtCost(t.avg_cost_usd), expCompareNote(c.avg_cost_usd, t.avg_cost_usd, { lowerIsBetter: true, sameThresholdPct: 0.05 })));
    }
    const billRows = feature === "profile" ? [
        expCompareTr("Avg cost / turn", fmtCost(c.avg_cost_usd), fmtCost(t.avg_cost_usd), expCompareNote(c.avg_cost_usd, t.avg_cost_usd, { lowerIsBetter: true, sameThresholdPct: 0.05 })),
        expCompareTr("Input tokens", expFmtTokCell(c.avg_input_tokens), expFmtTokCell(t.avg_input_tokens), expCompareNote(c.avg_input_tokens, t.avg_input_tokens, { lowerIsBetter: true })),
        expCompareTr("Output tokens", expFmtTokCell(c.avg_output_tokens), expFmtTokCell(t.avg_output_tokens), expCompareNote(c.avg_output_tokens, t.avg_output_tokens, { lowerIsBetter: true, sameThresholdPct: 0.1 })),
        expCompareTr("Cache read", expFmtCacheCell(c.avg_cache_read_tokens), expFmtCacheCell(t.avg_cache_read_tokens), expCompareNote(c.avg_cache_read_tokens, t.avg_cache_read_tokens, { lowerIsBetter: true, sameThresholdPct: 0.08 })),
    ] : [
        expCompareTr("Input tokens", expFmtTokCell(c.avg_input_tokens), expFmtTokCell(t.avg_input_tokens), "context only, not the verdict"),
        expCompareTr("Output tokens", expFmtTokCell(c.avg_output_tokens), expFmtTokCell(t.avg_output_tokens), expCompareNote(c.avg_output_tokens, t.avg_output_tokens, { lowerIsBetter: true, sameThresholdPct: 0.1 })),
        expCompareTr("Cache read", expFmtCacheCell(c.avg_cache_read_tokens), expFmtCacheCell(t.avg_cache_read_tokens), expCompareNote(c.avg_cache_read_tokens, t.avg_cache_read_tokens, { lowerIsBetter: true, sameThresholdPct: 0.08 })),
    ];
    const insight = expLearningInsight(f, t, c);
    return `${expKpiBanner(feature)}
      ${expCompareTable(kpi.ctxSection, ctxRows, c.count, t.count)}
      ${expCompareTable(feature === "profile" ? kpi.billSection + " (noisy early)" : kpi.billSection, billRows, c.count, t.count)}
      <p class="exp-compare-insight">${esc(insight)}</p>`;
}

function expFmtSavingsLine(withCost, withoutCost) {
    const saved = expPerPromptSavings(withCost, withoutCost);
    if (Math.abs(saved) < 0.005) return 'About the same cost with or without this feature';
    if (saved > 0) return 'Saves ' + expFmtCents(saved) + ' per prompt on average';
    return 'Costs ' + expFmtCents(Math.abs(saved)) + ' more per prompt on average';
}

function expFmtDiffCell(withCost, withoutCost) {
    const diff = expPerPromptSavings(withCost, withoutCost);
    if (Math.abs(diff) < 0.005) return 'Same';
    if (diff > 0) return 'Saves ' + expFmtCents(diff);
    return 'Costs ' + expFmtCents(Math.abs(diff)) + ' more';
}

function expConfidencePercent(withCount, withoutCount) {
    return Math.min(100, Math.round((Math.min(withCount, withoutCount) / 100) * 100));
}

function expPromptsNeeded(withCount, withoutCount) {
    return Math.max(0, 100 - Math.min(withCount, withoutCount));
}

function expFeatureVerdict(feature, t, c) {
    const withCount = t.count || 0;
    const withoutCount = c.count || 0;
    const total = withCount + withoutCount;
    if (total === 0) {
        return {
            kind: 'neutral',
            label: 'Not tested yet',
            headline: 'Waiting for your first prompt',
            recommendation: 'This feature is not part of the current test.',
        };
    }
    if (withCount < 100 || withoutCount < 100) {
        const need = expPromptsNeeded(withCount, withoutCount);
        const profileSavings = feature === 'profile' ? expProfileDirectSavingsLine(t) : null;
        return {
            kind: 'early',
            label: 'Too early to tell',
            headline: profileSavings ? 'Ctx is saving on tools when filtering is on' : need > 0 ? 'Need about ' + need + ' more prompts on each side before we can be confident' : 'Collecting prompts. Keep using Claude normally.',
            recommendation: profileSavings ? 'Keep the test running. Whole-turn cost needs about 100 prompts per side.' : total < 20 ? 'Keep using Claude normally. Results update every few minutes.' : 'Early signs only. Keep the test running.',
        };
    }
    if (t.avg_cost_usd <= 0 && c.avg_cost_usd <= 0) {
        return {
            kind: 'early',
            label: 'Too early to tell',
            headline: 'No cost data on compared prompts yet',
            recommendation: 'Costs appear after ctx ingests API usage (~5 min).',
        };
    }
    const delta = c.avg_cost_usd > 0 ? ((c.avg_cost_usd - t.avg_cost_usd) / c.avg_cost_usd) * 100 : 0;
    const label = EXP_FEATURE_LABELS[feature] || feature;
    if (Math.abs(delta) < 3) {
        return {
            kind: 'same',
            label: 'About the same',
            headline: 'About the same cost with or without this feature',
            recommendation: 'No strong reason to turn ' + label + ' off or on for cost alone.',
        };
    }
    if (delta > 0) {
        return {
            kind: 'saving',
            label: 'Saving you money',
            headline: expFmtSavingsLine(t.avg_cost_usd, c.avg_cost_usd),
            recommendation: 'Keep this on. It is cheaper on average.',
        };
    }
    return {
        kind: 'costing',
        label: 'Costing more',
        headline: expFmtSavingsLine(t.avg_cost_usd, c.avg_cost_usd),
        recommendation: 'Consider turning off. It costs slightly more on average.',
    };
}

function expIsTestActive(ab) {
    return ab.profile_pct < 100 || ab.inject_pct < 100 || ab.adaptive_pct < 100 || ab.coaching_pct < 100;
}

function expFmtTimeShort(ts) {
    const d = new Date(ts);
    return d.toLocaleTimeString('en-US', {
        hour: 'numeric',
        minute: '2-digit'
    });
}

function expParseAbEntry(code) {
    const m = String(code).match(/^([PIAC]):([TC])$/);
    if (!m) return null;
    return {
        feature: EXP_AB_CODE[m[1]] || m[1],
        on: m[2] === 'T',
    };
}

function expPrimaryAbEntry(abGroup) {
    if (!abGroup) return null;
    const parts = abGroup.split(/\s+/).filter(Boolean);
    for (const p of parts) {
        const parsed = expParseAbEntry(p);
        if (parsed) return parsed;
    }
    return null;
}

function expBuildHero(report, ab, active, enrichedCount) {
    if (!active && enrichedCount === 0) {
        return {
            headline: 'Find out which ctx features actually save you money',
            body: 'ctx measures two things: direct savings (tools stripped, tokens cut) and whole-turn API cost. The first is what ctx controls; the second is noisy until you have enough prompts.',
            action: 'Start a test on Profile filtering (50/50 split) or adjust gates in Settings.',
        };
    }
    const totalCompared = enrichedCount;
    const anyReady = report.some(f => f.treatment.count >= 100 && f.control.count >= 100);
    if (!anyReady) {
        const need = report.reduce((best, f) => {
            const n = expPromptsNeeded(f.treatment.count, f.control.count);
            return n > best ? n : best;
        }, 0);
        const activeNames = [];
        if (ab.profile_pct < 100) activeNames.push('Profile');
        if (ab.inject_pct < 100) activeNames.push('Inject');
        if (ab.adaptive_pct < 100) activeNames.push('Adaptive');
        if (ab.coaching_pct < 100) activeNames.push('Coaching');
        const testing = activeNames.length ? activeNames.join(', ') : 'features';
        return {
            headline: 'Still learning. Keep using Claude normally',
            body: testing + ' ' + (activeNames.length === 1 ? 'is' : 'are') + ' being tested on ' + totalCompared + ' enriched prompt' + (totalCompared !== 1 ? 's' : '') + ' since activation (Pipeline tab shows today only). Results update every few minutes after ctx ingests usage.',
            action: need > 0 ? 'About ' + need + ' more prompts on each side before we can recommend keep vs turn off.' : 'Keep working normally while ctx collects more data.',
        };
    }
    const ranked = report.filter(f => f.treatment.count >= 100 && f.control.count >= 100).map(f => ({
        feature: f.feature,
        label: EXP_FEATURE_LABELS[f.feature] || f.feature,
        saved: expPerPromptSavings(f.treatment.avg_cost_usd, f.control.avg_cost_usd),
    })).sort((a, b) => b.saved - a.saved);
    const best = ranked[0];
    const worst = ranked.filter(r => r.saved < -0.005).sort((a, b) => a.saved - b.saved)[0];
    let headline = best && best.saved > 0.005 ? best.label + ' is your best saver' : 'Your features are about equally costly';
    if (worst && worst.saved < -0.005) {
        headline += worst.feature !== best?.feature ? '; ' + worst.label + ' may not be worth it' : '';
    }
    const bodyParts = ranked.filter(r => Math.abs(r.saved) >= 0.005).slice(0, 3).map(r => {
        if (r.saved > 0) return r.label + ' saves ' + expFmtCents(r.saved) + ' per prompt';
        return r.label + ' costs ' + expFmtCents(Math.abs(r.saved)) + ' more per prompt';
    });
    const recParts = [];
    if (best && best.saved > 0.005) recParts.push('keep ' + best.label.toLowerCase() + ' on');
    if (worst && worst.saved < -0.005) recParts.push('review ' + worst.label.toLowerCase() + ' in Settings');
    return {
        headline,
        body: bodyParts.length ? bodyParts.join('. ') + '.' : 'No meaningful cost differences detected yet.',
        action: recParts.length ? 'Recommendation: ' + recParts.join('; ') + '.' : 'No strong recommendation. Features look similar on cost.',
    };
}

function expRenderHero(hero) {
    const el = document.getElementById('exp-hero');
    if (!el) return;
    el.innerHTML = ` <div class="narrative-eyebrow">Bottom line</div> <div class="narrative-body" style="margin-bottom:12px"><strong>${esc(hero.headline)}</strong></div> <div class="section-sub" style="margin-bottom:8px;line-height:1.55">${esc(hero.body)}</div> <div class="section-sub" style="margin-bottom:0;color:var(--t3)">${esc(hero.action)}</div>`;
}

function expRenderQuickActions(active) {
    const startBtn = document.getElementById('exp-btn-start');
    const stopBtn = document.getElementById('exp-btn-stop');
    if (startBtn) startBtn.disabled = !!active;
    if (stopBtn) stopBtn.disabled = !active;
}

function expRenderFeatureCard(f) {
    const t = f.treatment;
    const c = f.control;
    const verdict = expFeatureVerdict(f.feature, t, c);
    const label = EXP_FEATURE_LABELS[f.feature] || f.feature;
    const total = t.count + c.count;
    const confidence = expConfidencePercent(t.count, c.count);
    const hasData = total > 0;
    const need = expPromptsNeeded(t.count, c.count);
    const primary = expHeroPrimary(f.feature, t, c);
    const insight = hasData ? expLearningInsight(f, t, c) : verdict.recommendation;

    let body = "";
    if (hasData) {
        body += `<div class="exp-hero-primary exp-hero-${primary.tone}">
          <div class="exp-hero-primary-label">${esc(primary.title)}</div>
          <div class="exp-hero-primary-val">${esc(primary.value)}</div>
          <div class="exp-hero-primary-sub">${esc(primary.sub)}</div>
        </div>`;
        body += `<p class="exp-insight-short">${esc(insight)}</p>`;
        body += `<div class="exp-card-meta-row">
          <div class="exp-meta-chips">
            <span class="exp-meta-chip">${total.toLocaleString()} prompts since activation</span>
            <span class="exp-meta-chip exp-meta-on">${t.count} on</span>
            <span class="exp-meta-chip exp-meta-off">${c.count} off</span>
          </div>
          <p class="exp-meta-scope">Each feature flips its own on/off coin per prompt, so on/off counts differ across cards. Pipeline shows today only.</p>
          <div class="exp-meta-confidence">
            <span class="exp-meta-conf-label">${confidence >= 100 ? "Ready to decide" : confidence + "% toward 100 per side"}</span>
            <span class="exp-confidence-track"><span class="exp-confidence-fill" style="width:${confidence}%"></span></span>
          </div>
        </div>`;
        body += `<details class="exp-details exp-breakdown">
          <summary>Full breakdown</summary>
          <div class="exp-details-body">${expComparisonPanel(f, t, c)}</div>
        </details>`;
    } else {
        body += `<div class="exp-card-empty">${esc(verdict.recommendation)}</div>`;
    }

    const sub = hasData && need > 0
        ? `${need} more prompts per side for a solid read`
        : hasData
        ? `Measuring ${esc((EXP_FEATURE_KPI[f.feature] || EXP_FEATURE_KPI.profile).primaryName)}`
        : "";

    return `<div class="exp-feature-card exp-card-${verdict.kind}">
      <div class="exp-feature-card-top">
        <div class="exp-feature-name-row">
          <div class="exp-feature-name">${esc(label)}</div>
          <span class="exp-verdict-pill exp-verdict-${verdict.kind}">${esc(verdict.label)}</span>
        </div>
        ${sub ? `<div class="exp-feature-sub">${sub}</div>` : ""}
      </div>
      ${body}
    </div>`;
}

function expRenderRecentPrompts(traces) {
    const el = document.getElementById('exp-recent-prompts');
    if (!el) return;
    const expTraces = (traces || []).filter(h => h.ab_group);
    if (!expTraces.length) {
        el.innerHTML = '<div class="empty" style="padding:24px">No prompts in the test yet. Send Claude Code traffic while a test is running.</div>';
        return;
    }
    el.innerHTML = expTraces.slice(0, 12).map(ht => {
        const entry = expPrimaryAbEntry(ht.ab_group);
        const feature = entry ? (EXP_FEATURE_LABELS[entry.feature] || entry.feature) : 'Test';
        const on = entry ? entry.on : false;
        const cost = ht.enriched && ht.cost_usd ? expFmtCents(ht.cost_usd) : '…';
        const prompt = ht.human_text_prefix ? esc(ht.human_text_prefix).slice(0, 80) + (ht.human_text_prefix.length > 80 ? '…' : '') : '<span style="color:var(--t4)">(prompt text not stored)</span>';
        return `<div class="exp-prompt-row"> <span class="exp-prompt-time">${esc(expFmtTimeShort(ht.ts))}</span> <span class="exp-prompt-feature">${esc(feature)}</span> <span class="exp-on-pill ${on ? 'on' : 'off'}">${on ? 'On' : 'Off'}</span> <span class="exp-prompt-cost">${cost}</span> <span class="exp-prompt-text">${prompt}</span> </div>`;
    }).join('');
}

function expRenderDailyTable(featureId) {
    const el = document.getElementById('exp-daily-table');
    if (!el || !_abDailyRows.length) {
        if (el) el.innerHTML = '<div class="empty" style="padding:16px">No daily breakdown yet.</div>';
        return;
    }
    const byDate = {};
    _abDailyRows.forEach(r => {
        if (r.feature !== featureId) return;
        if (!byDate[r.date]) byDate[r.date] = {};
        byDate[r.date][r.group] = r;
    });
    const dates = Object.keys(byDate).sort().reverse();
    if (!dates.length) {
        el.innerHTML = '<div class="empty" style="padding:16px">No daily rows for this feature yet.</div>';
        return;
    }
    let html = '<table class="exp-daily-table"><thead><tr><th>Day</th><th>On</th><th>Off</th><th>Avg cost (on)</th><th>Avg cost (off)</th><th>Difference</th></tr></thead><tbody>';
    dates.forEach(d => {
        const withRow = byDate[d].treatment;
        const withoutRow = byDate[d].control;
        const withCost = withRow ? withRow.avg_cost : 0;
        const withoutCost = withoutRow ? withoutRow.avg_cost : 0;
        const diff = expPerPromptSavings(withCost, withoutCost);
        const rowCls = diff > 0.005 ? 'exp-row-saving' : diff < -0.005 ? 'exp-row-costing' : '';
        html += `<tr class="${rowCls}"> <td>${esc(d)}</td> <td>${withRow ? withRow.count + ' prompts' : EXP_EMPTY}</td> <td>${withoutRow ? withoutRow.count + ' prompts' : EXP_EMPTY}</td> <td>${withRow ? fmtCost(withCost) : EXP_EMPTY}</td> <td>${withoutRow ? fmtCost(withoutCost) : EXP_EMPTY}</td> <td>${withRow && withoutRow ? esc(expFmtDiffCell(withCost, withoutCost)) : EXP_EMPTY}</td> </tr>`;
    });
    html += '</tbody></table>';
    el.innerHTML = html;
}

function renderAbTrendChart() {
    const feat = document.getElementById('exp-trend-feature')?.value || 'profile';
    expRenderDailyTable(feat);
    const canvas = document.getElementById('exp-trend-chart');
    if (!canvas || typeof Chart === 'undefined') return;
    const labels = [...new Set(_abDailyRows.filter(r => r.feature === feat).map(r => r.date))].sort();
    const withPts = labels.map(d => {
        const r = _abDailyRows.find(x => x.date === d && x.feature === feat && x.group === 'treatment');
        return r ? r.avg_cost : null;
    });
    const withoutPts = labels.map(d => {
        const r = _abDailyRows.find(x => x.date === d && x.feature === feat && x.group === 'control');
        return r ? r.avg_cost : null;
    });
    if (_abTrendChart) _abTrendChart.destroy();
    if (!labels.length) return;
    _abTrendChart = new Chart(canvas, {
        type: 'line',
        data: {
            labels,
            datasets: [{
                label: 'Avg cost (on)',
                data: withPts,
                borderColor: '#86efac',
                tension: 0.2
            }, {
                label: 'Avg cost (off)',
                data: withoutPts,
                borderColor: '#fdba74',
                tension: 0.2
            }, ],
        },
        options: {
            responsive: true,
            plugins: {
                legend: {
                    labels: {
                        color: '#94a3b8'
                    }
                }
            },
            scales: {
                x: {
                    ticks: {
                        color: '#64748b'
                    },
                    title: {
                        display: true,
                        text: 'Day',
                        color: '#64748b'
                    }
                },
                y: {
                    ticks: {
                        color: '#64748b'
                    },
                    title: {
                        display: true,
                        text: 'Avg cost (USD)',
                        color: '#64748b'
                    }
                },
            },
        },
    });
}

function onExpTrendFeatureChange() {
    renderAbTrendChart();
}
async function expStartProfileTest() {
    const s = await fetch('/api/settings').then(r => r.json());
    const ab = s.ab_test || {};
    await fetch('/api/settings', {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json'
        },
        body: JSON.stringify({
            ab_test: {
                ...ab,
                profile_pct: 50
            }
        }),
    });
    await loadExperimentTab();
}
async function expStopTest() {
    await fetch('/api/settings', {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json'
        },
        body: JSON.stringify({
            ab_test: {
                profile_pct: 100,
                inject_pct: 100,
                adaptive_pct: 100,
                coaching_pct: 100
            },
        }),
    });
    await loadExperimentTab();
}

function expOpenSettings() {
    const nav = document.querySelector('.nav-item[onclick*="settings"]');
    if (nav) showTab('settings', nav);
}
async function loadExperimentTab() {
    const idle = document.getElementById('exp-idle');
    const body = document.getElementById('exp-body');
    const status = document.getElementById('exp-status');
    try {
        if (status) status.style.display = 'none';
        const settings = await fetch('/api/settings').then(r => r.json());
        const ab = settings.ab_test || {};
        _expAbActive = expIsTestActive(ab);
        expRenderQuickActions(_expAbActive);
        const report = await fetch(appendSince('/api/ab-report')).then(r => r.json());
        _abDailyRows = await fetch(appendSince('/api/ab-daily')).then(r => r.json());
        const enrichedCount = report.reduce((s, f) => s + f.treatment.count + f.control.count, 0);
        const hasAbRows = enrichedCount > 0;
        const hero = expBuildHero(report, ab, _expAbActive, enrichedCount);
        expRenderHero(hero);
        if (!_expAbActive && !hasAbRows) {
            if (idle) idle.style.display = 'block';
            if (body) body.style.display = 'none';
            return;
        }
        if (idle) idle.style.display = 'none';
        if (body) body.style.display = 'block';
        const grid = document.getElementById('exp-feature-grid');
        if (grid) grid.innerHTML = report.map(expRenderFeatureCard).join('');
        renderAbTrendChart();
        const traces = await fetch(appendSince('/api/hook-traces?limit=50')).then(r => r.json());
        expRenderRecentPrompts(traces);
    } catch (e) {
        if (status) {
            status.style.display = 'block';
            status.textContent = 'Could not load experiment data: ' + e;
        }
        if (idle) idle.style.display = 'none';
        if (body) body.style.display = 'none';
    }
}