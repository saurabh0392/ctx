//! Small, dependency-free proportion statistics shared by the causal before/after report
//! (`ctx context proof`) and the activation gate (`compress::activation`). Keeping these in
//! one place means the gate and the report always agree on what "trimming is safe" means.

/// 95% Wilson score interval for `k` successes in `n` trials, returned as (low, high) in
/// the 0..1 range. Honest small-sample behavior: with n=0 it returns the whole (0,1).
pub fn wilson_interval(k: i64, n: i64) -> (f64, f64) {
    if n <= 0 {
        return (0.0, 1.0);
    }
    let z = 1.96_f64;
    let n = n as f64;
    let p = k as f64 / n;
    let z2 = z * z;
    let denom = 1.0 + z2 / n;
    let center = (p + z2 / (2.0 * n)) / denom;
    let margin = (z / denom) * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt();
    ((center - margin).max(0.0), (center + margin).min(1.0))
}

/// Newcombe's interval for the difference (p_trimmed - p_baseline), built from each arm's
/// Wilson interval. This is the honest answer to "did trimming move the rate, and by how
/// much, with what uncertainty". Returns (delta, low, high).
pub fn newcombe_diff(k1: i64, n1: i64, k2: i64, n2: i64) -> (f64, f64, f64) {
    let p1 = if n1 > 0 { k1 as f64 / n1 as f64 } else { 0.0 };
    let p2 = if n2 > 0 { k2 as f64 / n2 as f64 } else { 0.0 };
    let (l1, u1) = wilson_interval(k1, n1);
    let (l2, u2) = wilson_interval(k2, n2);
    let delta = p1 - p2;
    let low = delta - ((p1 - l1).powi(2) + (u2 - p2).powi(2)).sqrt();
    let high = delta + ((u1 - p1).powi(2) + (p2 - l2).powi(2)).sqrt();
    (delta, low, high)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wilson_zero_n_is_full_range() {
        assert_eq!(wilson_interval(0, 0), (0.0, 1.0));
    }

    #[test]
    fn wilson_brackets_point_estimate() {
        let (lo, hi) = wilson_interval(5, 100);
        assert!(lo < 0.05 && 0.05 < hi, "interval {lo}..{hi} must bracket 0.05");
        assert!(lo >= 0.0 && hi <= 1.0);
    }

    #[test]
    fn newcombe_sign_follows_delta() {
        // trimmed worse than baseline -> positive delta
        let (d, _, _) = newcombe_diff(20, 100, 10, 100);
        assert!(d > 0.0);
        // trimmed better -> negative delta
        let (d2, _, _) = newcombe_diff(5, 100, 15, 100);
        assert!(d2 < 0.0);
    }
}
