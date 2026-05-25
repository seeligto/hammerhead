"""Phase 28F-2 sub-phase 0 — analyze eval_comparison.csv.

Reads eval_comparison.csv, computes the 6 analysis sections, writes
analysis.md + gate_verdict.md.
"""

from __future__ import annotations

import csv
import math
import sys
from collections import defaultdict
from pathlib import Path
from statistics import mean, median, stdev

CSV_PATH = Path("/tmp/phase_28f/2/0/eval_comparison.csv")
OUT_ANALYSIS = Path("/tmp/phase_28f/2/0/analysis.md")
OUT_VERDICT = Path("/tmp/phase_28f/2/0/gate_verdict.md")


def sign(x: float) -> int:
    if x > 0:
        return 1
    if x < 0:
        return -1
    return 0


def pearson(xs: list[float], ys: list[float]) -> float:
    if len(xs) < 2:
        return float("nan")
    mx = mean(xs)
    my = mean(ys)
    num = sum((x - mx) * (y - my) for x, y in zip(xs, ys))
    dx = math.sqrt(sum((x - mx) ** 2 for x in xs))
    dy = math.sqrt(sum((y - my) ** 2 for y in ys))
    if dx == 0 or dy == 0:
        return float("nan")
    return num / (dx * dy)


def load() -> list[dict]:
    with open(CSV_PATH) as f:
        rows = list(csv.DictReader(f))
    out = []
    for r in rows:
        try:
            r["hh_eval"] = float(r["hh_eval"]) if r["hh_eval"] else None
            r["sb_eval"] = float(r["sb_eval"]) if r["sb_eval"] else None
            r["hh_layer1"] = int(r["hh_layer1"]) if r["hh_layer1"] else 0
            r["hh_layer2"] = int(r["hh_layer2"]) if r["hh_layer2"] else 0
            r["hh_layer3"] = int(r["hh_layer3"]) if r["hh_layer3"] else 0
            r["stone_count"] = int(r["stone_count"])
            out.append(r)
        except Exception:
            continue
    return out


def section_sign_agreement(rows: list[dict]) -> tuple[dict, str]:
    """For each row, do HH and SB agree on the winning side?"""
    by_cat = defaultdict(lambda: {"total": 0, "agree": 0, "both_zero": 0})
    overall = {"total": 0, "agree": 0, "both_zero": 0}
    by_label = defaultdict(lambda: {"total": 0, "agree": 0})

    for r in rows:
        if r["hh_eval"] is None or r["sb_eval"] is None:
            continue
        sh = sign(r["hh_eval"])
        ss = sign(r["sb_eval"])
        cat = r["category"]
        label = r["position_label"]
        for d in (by_cat[cat], overall):
            d["total"] += 1
            if sh == 0 and ss == 0:
                d["both_zero"] += 1
                d["agree"] += 1
            elif sh == ss:
                d["agree"] += 1
        by_label[label]["total"] += 1
        if sh == ss:
            by_label[label]["agree"] += 1

    def rate(d):
        return d["agree"] / d["total"] if d["total"] else float("nan")

    lines = ["## 1. Sign agreement\n"]
    lines.append(f"**Overall**: {overall['agree']}/{overall['total']} = "
                 f"{rate(overall)*100:.1f}% (both-zero: {overall['both_zero']})")
    lines.append("\nBy category:\n")
    lines.append("| Category | Agree | Total | Rate |")
    lines.append("|---|---:|---:|---:|")
    for cat in sorted(by_cat.keys()):
        d = by_cat[cat]
        lines.append(f"| {cat} | {d['agree']} | {d['total']} | {rate(d)*100:.1f}% |")
    lines.append("\nBy position label:\n")
    lines.append("| Label | Agree | Total | Rate |")
    lines.append("|---|---:|---:|---:|")
    for label in ["stone10", "stone15", "stone20", "decisive"]:
        if label not in by_label:
            continue
        d = by_label[label]
        lines.append(f"| {label} | {d['agree']} | {d['total']} | {rate(d)*100:.1f}% |")
    return {"overall_rate": rate(overall), "by_cat": {c: rate(d) for c, d in by_cat.items()}}, "\n".join(lines)


def section_magnitude_corr(rows: list[dict]) -> tuple[dict, str]:
    """Pearson r overall + per category. Strip mate-class scores."""
    MATE_THRESH = 900_000

    def clean(rs):
        xs, ys = [], []
        for r in rs:
            if r["hh_eval"] is None or r["sb_eval"] is None:
                continue
            if abs(r["hh_eval"]) >= MATE_THRESH or abs(r["sb_eval"]) >= MATE_THRESH:
                continue
            xs.append(r["hh_eval"])
            ys.append(r["sb_eval"])
        return xs, ys

    all_x, all_y = clean(rows)
    r_overall = pearson(all_x, all_y)

    by_cat = {}
    for cat in {r["category"] for r in rows}:
        cs = [r for r in rows if r["category"] == cat]
        xs, ys = clean(cs)
        by_cat[cat] = (pearson(xs, ys), len(xs))

    lines = ["## 2. Magnitude correlation (Pearson, mate-class excluded)\n"]
    lines.append(f"**Overall**: r = {r_overall:.3f} (n={len(all_x)})\n")
    lines.append("| Category | r | n |")
    lines.append("|---|---:|---:|")
    for cat in sorted(by_cat.keys()):
        r, n = by_cat[cat]
        lines.append(f"| {cat} | {r:.3f} | {n} |")
    return {"overall_r": r_overall, "by_cat": by_cat}, "\n".join(lines)


def section_divergence_outcome(rows: list[dict]) -> tuple[dict, str]:
    """For positions where HH/SB disagree in sign, which engine matches
    the actual game outcome?"""
    overall = {"divergent": 0, "sb_right": 0, "hh_right": 0, "tie": 0}
    by_cat = defaultdict(lambda: {"divergent": 0, "sb_right": 0, "hh_right": 0, "tie": 0})

    for r in rows:
        if r["hh_eval"] is None or r["sb_eval"] is None:
            continue
        sh = sign(r["hh_eval"])
        ss = sign(r["sb_eval"])
        if sh == 0 or ss == 0 or sh == ss:
            continue  # not divergent
        outcome = r["outcome"]  # 'X' or 'O'
        outcome_sign = 1 if outcome == "X" else (-1 if outcome == "O" else 0)
        if outcome_sign == 0:
            continue
        for d in (overall, by_cat[r["category"]]):
            d["divergent"] += 1
            if ss == outcome_sign and sh != outcome_sign:
                d["sb_right"] += 1
            elif sh == outcome_sign and ss != outcome_sign:
                d["hh_right"] += 1
            else:
                d["tie"] += 1

    def rate(d):
        return d["sb_right"] / d["divergent"] if d["divergent"] else float("nan")

    lines = ["## 3. Divergence-vs-outcome\n"]
    lines.append(f"**Overall**: {overall['divergent']} divergent positions. "
                 f"SB right: {overall['sb_right']} ({rate(overall)*100:.1f}%), "
                 f"HH right: {overall['hh_right']}, tie: {overall['tie']}\n")
    lines.append("| Category | Divergent | SB right | HH right | SB hit-rate |")
    lines.append("|---|---:|---:|---:|---:|")
    for cat in sorted(by_cat.keys()):
        d = by_cat[cat]
        lines.append(f"| {cat} | {d['divergent']} | {d['sb_right']} | {d['hh_right']} | "
                     f"{rate(d)*100:.1f}% |")
    return {"overall_sb_hit": rate(overall), "by_cat": by_cat}, "\n".join(lines)


def section_per_category(rows: list[dict], hh_side_map: dict[str, str]) -> str:
    """For each cat, show HH-perspective HH vs SB means at decisive
    and at stone20. Positive = HH sees self winning. Mate-class scores
    clamped to ±1.0 to dominate-by-count, not by magnitude."""
    MATE_THRESH = 900_000
    MATE_CLAMP = 1_000_000  # display value for mate-class

    def hh_persp(r, val):
        side = hh_side_map.get(r["game_id"], "X")
        return val if side == "X" else -val

    def clamp_mate(v):
        if v >= MATE_THRESH:
            return MATE_CLAMP
        if v <= -MATE_THRESH:
            return -MATE_CLAMP
        return v

    by_cat_label = defaultdict(lambda: defaultdict(list))
    for r in rows:
        if r["hh_eval"] is None or r["sb_eval"] is None:
            continue
        by_cat_label[r["category"]][r["position_label"]].append(r)

    lines = ["## 4. Per-category split (HH-perspective, mate-class clamped to ±1e6)\n"]
    lines.append("| Category | Label | n | HH mean | SB mean | HH median | SB median | HH-SB diff |")
    lines.append("|---|---|---:|---:|---:|---:|---:|---:|")
    for cat in sorted(by_cat_label.keys()):
        for label in ["stone10", "stone15", "stone20", "decisive"]:
            rs = by_cat_label[cat].get(label, [])
            if not rs:
                continue
            hh_vals = [clamp_mate(hh_persp(r, r["hh_eval"])) for r in rs]
            sb_vals = [clamp_mate(hh_persp(r, r["sb_eval"])) for r in rs]
            hh_m = mean(hh_vals)
            sb_m = mean(sb_vals)
            hh_med = median(hh_vals)
            sb_med = median(sb_vals)
            diff = hh_m - sb_m
            lines.append(f"| {cat} | {label} | {len(rs)} | {hh_m:.0f} | {sb_m:.0f} | "
                         f"{hh_med:.0f} | {sb_med:.0f} | {diff:.0f} |")
    return "\n".join(lines)


def section_layer_decomp(rows: list[dict], hh_side_map: dict[str, str]) -> tuple[dict, str]:
    """Layer breakdown for CROSS-AXIS-CLUSTER decisive positions, in
    HH-perspective sign (positive => good for HH)."""
    cat_rows = [r for r in rows if r["category"] == "CROSS-AXIS-CLUSTER"
                and r["position_label"] == "decisive"]
    if not cat_rows:
        return {}, "## 5. Layer decomposition\n\nNo CROSS-AXIS-CLUSTER decisive rows.\n"

    # Convert layer values to HH-perspective (positive = good for HH).
    def hh_persp(r, val):
        side = hh_side_map.get(r["game_id"], "X")
        return val if side == "X" else -val

    l1s = [hh_persp(r, r["hh_layer1"]) for r in cat_rows]
    l2s = [hh_persp(r, r["hh_layer2"]) for r in cat_rows]
    l3s = []
    for r in cat_rows:
        v = r["hh_layer3"]
        # Skip mate-class sentinels.
        if abs(v) >= 1_000_000_000:
            continue
        l3s.append(hh_persp(r, v))

    def stats(xs):
        if not xs:
            return (float("nan"),) * 4
        return (mean(xs), median(xs), min(xs), max(xs))

    m1, med1, lo1, hi1 = stats(l1s)
    m2, med2, lo2, hi2 = stats(l2s)
    m3, med3, lo3, hi3 = stats(l3s)

    def sign_split(xs):
        pos = sum(1 for x in xs if x > 0)
        neg = sum(1 for x in xs if x < 0)
        zer = sum(1 for x in xs if x == 0)
        return pos, neg, zer

    p1, n1, z1 = sign_split(l1s)
    p2, n2, z2 = sign_split(l2s)
    p3, n3, z3 = sign_split(l3s)

    # Key diagnostic: Layer-1 HELP / HURT / NEUTRAL.
    # Layer-1 HURTS if positive on cluster positions (false hope) when
    # Layer-2 is correctly negative.
    contradict = 0  # l1 says HH winning, l2 says HH losing
    aligned = 0
    for r in cat_rows:
        side = hh_side_map.get(r["game_id"], "X")
        l1_hh = r["hh_layer1"] if side == "X" else -r["hh_layer1"]
        l2_hh = r["hh_layer2"] if side == "X" else -r["hh_layer2"]
        if l1_hh > 0 and l2_hh < 0:
            contradict += 1
        elif l1_hh < 0 and l2_hh < 0:
            aligned += 1

    pct_contra = contradict / len(cat_rows) * 100 if cat_rows else 0
    pct_aligned = aligned / len(cat_rows) * 100 if cat_rows else 0

    if pct_contra >= 30:
        verdict = "HURT (Layer-1 generates false-positive signal vs Layer-2)"
    elif m1 > 0 and m2 < 0:
        verdict = "HURT (Layer-1 mean positive vs Layer-2 mean negative)"
    elif abs(m1) < abs(m2) * 0.1:
        verdict = "NEUTRAL (Layer-1 magnitude < 10% of Layer-2)"
    elif (m1 < 0) == (m2 < 0):
        verdict = "HELP (Layer-1 sign aligns with Layer-2)"
    else:
        verdict = "MIXED"

    lines = ["## 5. Layer decomposition (CROSS-AXIS-CLUSTER, decisive, HH-perspective)\n"]
    lines.append(f"n = {len(cat_rows)} cluster-decisive positions\n")
    lines.append("| Layer | Mean | Median | Min | Max | Positive | Negative | Zero |")
    lines.append("|---|---:|---:|---:|---:|---:|---:|---:|")
    lines.append(f"| Layer-1 | {m1:.0f} | {med1:.0f} | {lo1:.0f} | {hi1:.0f} | "
                 f"{p1} | {n1} | {z1} |")
    lines.append(f"| Layer-2 | {m2:.0f} | {med2:.0f} | {lo2:.0f} | {hi2:.0f} | "
                 f"{p2} | {n2} | {z2} |")
    lines.append(f"| Layer-3 | {m3:.0f} | {med3:.0f} | {lo3:.0f} | {hi3:.0f} | "
                 f"{p3} | {n3} | {z3} |")
    lines.append(f"\n**L1-vs-L2 contradiction** (L1 positive, L2 negative): {contradict} "
                 f"({pct_contra:.1f}%)")
    lines.append(f"**L1-vs-L2 alignment** (both negative): {aligned} ({pct_aligned:.1f}%)\n")
    lines.append(f"**Verdict: Layer-1 = {verdict}**\n")

    return {
        "l1_mean": m1, "l2_mean": m2, "l3_mean": m3,
        "contradict_pct": pct_contra,
        "verdict": verdict,
    }, "\n".join(lines)


def section_trajectory(rows: list[dict]) -> str:
    """Sign-agreement and mean-diff by position label."""
    by_label = defaultdict(list)
    for r in rows:
        if r["hh_eval"] is None or r["sb_eval"] is None:
            continue
        by_label[r["position_label"]].append(r)

    lines = ["## 6. Eval trajectory\n"]
    lines.append("Sign-agreement + mean abs diff by sample position:\n")
    lines.append("| Label | n | Sign agree % | HH mean | SB mean | |HH-SB| mean (non-mate) |")
    lines.append("|---|---:|---:|---:|---:|---:|")
    for label in ["stone10", "stone15", "stone20", "decisive"]:
        if label not in by_label:
            continue
        rs = by_label[label]
        agree = sum(1 for r in rs if sign(r["hh_eval"]) == sign(r["sb_eval"]))
        rate = agree / len(rs) * 100 if rs else 0
        non_mate = [r for r in rs
                    if abs(r["hh_eval"]) < 900_000 and abs(r["sb_eval"]) < 900_000]
        if non_mate:
            hh_mean = mean(r["hh_eval"] for r in non_mate)
            sb_mean = mean(r["sb_eval"] for r in non_mate)
            abs_diff = mean(abs(r["hh_eval"] - r["sb_eval"]) for r in non_mate)
        else:
            hh_mean = sb_mean = abs_diff = float("nan")
        lines.append(f"| {label} | {len(rs)} | {rate:.1f}% | {hh_mean:.0f} | {sb_mean:.0f} | "
                     f"{abs_diff:.0f} |")

    # First-divergence-stone: per game, find earliest sample where HH and SB
    # signs disagree.
    by_game = defaultdict(list)
    for r in rows:
        by_game[r["game_id"]].append(r)
    diverge_counts = {"stone10": 0, "stone15": 0, "stone20": 0, "decisive": 0, "never": 0}
    for gid, rs in by_game.items():
        rs_sorted = sorted(rs, key=lambda r: r["stone_count"])
        found = False
        for r in rs_sorted:
            if r["hh_eval"] is None or r["sb_eval"] is None:
                continue
            if sign(r["hh_eval"]) != sign(r["sb_eval"]):
                diverge_counts[r["position_label"]] = diverge_counts.get(r["position_label"], 0) + 1
                found = True
                break
        if not found:
            diverge_counts["never"] += 1
    lines.append("\nFirst-divergence position (per game):\n")
    lines.append("| Label | Games |")
    lines.append("|---|---:|")
    for label in ["stone10", "stone15", "stone20", "decisive", "never"]:
        lines.append(f"| {label} | {diverge_counts.get(label, 0)} |")
    return "\n".join(lines)


def build_hh_side_map() -> dict[str, str]:
    """Read classification.csv to map game_id → hh_side."""
    m = {}
    with open("/tmp/phase_28f/1/A/classification.csv") as f:
        for r in csv.DictReader(f):
            m[r["game_id"]] = r["hh_side"]
    return m


def main():
    rows = load()
    sys.stderr.write(f"loaded {len(rows)} rows\n")

    hh_side_map = build_hh_side_map()

    sign_data, sign_md = section_sign_agreement(rows)
    corr_data, corr_md = section_magnitude_corr(rows)
    div_data, div_md = section_divergence_outcome(rows)
    cat_md = section_per_category(rows, hh_side_map)
    layer_data, layer_md = section_layer_decomp(rows, hh_side_map)
    traj_md = section_trajectory(rows)

    analysis = "\n\n".join([
        "# Phase 28F-2 sub-phase 0 — Eval comparison analysis\n",
        sign_md, corr_md, div_md, cat_md, layer_md, traj_md,
    ])
    OUT_ANALYSIS.write_text(analysis + "\n")

    # ── Gate verdict ──
    cluster_sb_hit = float("nan")
    if "CROSS-AXIS-CLUSTER" in div_data["by_cat"]:
        d = div_data["by_cat"]["CROSS-AXIS-CLUSTER"]
        cluster_sb_hit = d["sb_right"] / d["divergent"] if d["divergent"] else float("nan")
    overall_sign_rate = sign_data["overall_rate"]
    overall_r = corr_data["overall_r"]
    overall_sb_hit = div_data["overall_sb_hit"]
    layer_verdict = layer_data.get("verdict", "UNKNOWN") if layer_data else "UNKNOWN"

    if overall_sign_rate < 0.6:
        verdict = "EVAL-SIGN"
        rationale = (f"Overall sign-agreement {overall_sign_rate*100:.1f}% < 60%: HH and SB "
                     "fundamentally disagree about who's winning. Codebook rewrite scope.")
    elif "HURT" in layer_verdict:
        verdict = "EVAL-LAYER1"
        rationale = (f"Layer-1 verdict: {layer_verdict}. Layer-1 generates false signal on "
                     "cluster positions → codebook rewrite (gamma) targets the right layer.")
    elif overall_sign_rate >= 0.8 and (math.isnan(cluster_sb_hit) or 0.40 <= cluster_sb_hit <= 0.60):
        # Sign agreement is high and on the disagreement set neither
        # eval reliably outperforms — gap is unlikely eval-side.
        verdict = "SEARCH-GAP"
        rationale = (f"Sign agreement {overall_sign_rate*100:.1f}% (>=80%); "
                     f"on the divergent cluster positions SB is right "
                     f"{cluster_sb_hit*100:.1f}% (coin flip). "
                     f"Layer-1 = {layer_verdict}. "
                     f"Pearson r={overall_r:.2f} is low — evals differ in *magnitude* "
                     "calibration but not in who's winning. HH eval is no worse than SB's "
                     "where they disagree, so adding eval (gamma codebook) won't close "
                     "the gap. Pivot to search-side: time mgmt, ordering, depth.")
    elif overall_sign_rate >= 0.8 and not math.isnan(cluster_sb_hit) and cluster_sb_hit >= 0.70:
        verdict = "EVAL-LAYER2"
        rationale = (f"Sign agreement {overall_sign_rate*100:.1f}% high but on divergent "
                     f"cluster positions SB is right {cluster_sb_hit*100:.1f}% (≥70%). "
                     "SB's eval is systematically better on cluster shapes → "
                     "Layer-2/3 shape detection needs enrichment.")
    else:
        verdict = "MIXED"
        rationale = (f"Mixed signal. sign={overall_sign_rate*100:.1f}%, r={overall_r:.2f}, "
                     f"SB-right={overall_sb_hit*100:.1f}%, layer={layer_verdict}.")

    vlines = [
        "# Phase 28F-2 sub-phase 0 — Gate verdict\n",
        f"**Verdict: {verdict}**\n",
        rationale,
        "",
        "## Key numbers",
        f"- Overall sign agreement: {overall_sign_rate*100:.1f}%",
        f"- Overall magnitude Pearson r (non-mate): {overall_r:.3f}",
        f"- Divergence-vs-outcome SB hit-rate (overall): "
        f"{(overall_sb_hit*100 if not math.isnan(overall_sb_hit) else float('nan')):.1f}%",
        f"- Divergence-vs-outcome SB hit-rate (CROSS-AXIS-CLUSTER): "
        f"{(cluster_sb_hit*100 if not math.isnan(cluster_sb_hit) else float('nan')):.1f}%",
        f"- Layer-1 verdict on cluster positions: {layer_verdict}",
        "",
        "## Sources",
        f"- /tmp/phase_28f/2/0/eval_comparison.csv ({len(rows)} rows)",
        "- /tmp/phase_28f/1/A/classification.csv (192 games)",
    ]
    OUT_VERDICT.write_text("\n".join(vlines) + "\n")

    print(verdict)
    print(rationale)


if __name__ == "__main__":
    main()
