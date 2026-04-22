use serde::{Deserialize, Serialize};

/// Paramètres d'entrée décrivant le tournoi que l'utilisateur veut organiser.
///
/// L'utilisateur décrit la **malette** (quantité totale de jetons par valeur)
/// et le nombre de joueurs. L'algorithme :
///   - ancre la **première blind** à la plus petite dénomination de la malette,
///   - ne distribue que les **2 plus petites dénominations** dans le stack de
///     départ (les grosses coupures sont réservées aux recolorages),
///   - vise une **profondeur** (en big blinds) dynamique dépendant de la durée
///     totale et du nombre de joueurs,
///   - génère la progression des blinds sur une échelle canonique (WSOP/WPT),
///     SB:BB toujours 1:2.
#[derive(Debug, Deserialize, Clone)]
pub struct TournamentInput {
    /// Nombre de joueurs au départ.
    pub players: u32,
    /// Durée cible totale du tournoi, en minutes.
    pub total_duration_minutes: u32,
    /// Contenu total de la malette — valeur faciale => quantité dans la malette.
    pub case_chips: Vec<ChipDenomination>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChipDenomination {
    pub value: u32,
    pub count: u32,
}

/// Un niveau de blind dans la structure générée.
#[derive(Debug, Serialize, Clone)]
pub struct BlindLevel {
    pub level: u32,
    pub small_blind: u32,
    pub big_blind: u32,
    pub ante: u32,
    pub duration_minutes: u32,
    pub is_break: bool,
}

#[derive(Debug, Serialize)]
pub struct TournamentStructure {
    /// Répartition par joueur calculée par l'algorithme (valeur => nb jetons).
    pub chips_per_player: Vec<ChipDenomination>,
    /// Valeur totale du stack de départ de chaque joueur.
    pub starting_stack: u32,
    /// Valeur totale en jeu (starting_stack * players).
    pub total_chips: u32,
    /// Durée de niveau choisie automatiquement, en minutes.
    pub level_duration_minutes: u32,
    pub number_of_levels: u32,
    pub levels: Vec<BlindLevel>,
}

/// Échelle canonique des SB, exprimée en **unités du plus petit jeton**.
const SB_LADDER_UNITS: &[u32] = &[
    1, 2, 3, 4, 6, 8, 10, 12, 15, 20, 25, 30, 40, 50, 60, 75, 100, 125, 150, 200, 250, 300, 400,
    500, 600, 750, 1000, 1250, 1500, 2000, 2500, 3000, 4000, 5000, 6000, 8000, 10000, 12500, 15000,
    20000, 25000, 30000, 40000, 50000,
];

/// Profondeur minimale/maximale cible par la formule de `target_depth_bb`.
/// Les profondeurs sont toujours arrondies à un multiple de [`DEPTH_STEP_BB`]
/// (ex. 100, 110, 120, …, 150 BB).
const MIN_DEPTH_BB: u32 = 100;
const MAX_DEPTH_BB: u32 = 150;
const DEPTH_STEP_BB: u32 = 10;
/// Plancher absolu en BB quand la malette ne peut pas fournir
/// [`MIN_DEPTH_BB`] : on décrémente par pas de 10 BB jusqu'à trouver une cible
/// atteignable, mais jamais en dessous de ce plancher.
const FLOOR_DEPTH_BB: u32 = 20;

/// Nombre maximal de dénominations distribuées dans le stack de départ.
/// Au-delà, les dénominations plus grosses sont réservées aux recolorages.
const MAX_DENOMS_IN_STACK: usize = 4;

/// Nombre minimal de jetons v1 gardés par joueur (pour payer SB/BB bas).
const MIN_V1_COUNT: u32 = 4;

pub fn compute_structure(input: &TournamentInput) -> TournamentStructure {
    let players = input.players.max(1);

    // 1. Tri des dénominations par valeur croissante, filtre des invalides.
    let mut denoms: Vec<&ChipDenomination> = input
        .case_chips
        .iter()
        .filter(|c| c.value > 0 && c.count > 0)
        .collect();
    denoms.sort_by_key(|c| c.value);

    // 2. Profondeur cible dynamique (en BB), bornée.
    let depth_bb = target_depth_bb(players, input.total_duration_minutes);

    // 3. Alloue le stack à partir des 2 plus petites dénominations, en visant
    //    `depth_bb × bb1`, borné par les dispos de la malette.
    let (chips_per_player, starting_stack, bb1) =
        allocate_stack(&denoms, players, depth_bb);

    let total_chips = starting_stack.saturating_mul(players);

    // 4. Durée de niveau (conservée : dépend de la durée totale).
    let level_duration_minutes = pick_level_duration(input.total_duration_minutes);
    let target_levels = (input.total_duration_minutes / level_duration_minutes).max(1);

    // 5. Unité = plus petit jeton distribué ; sinon bb1/2 en secours.
    let unit = chips_per_player
        .iter()
        .map(|c| c.value)
        .min()
        .filter(|&v| v > 0)
        .unwrap_or((bb1 / 2).max(1));

    // 6. SB de départ forcée à v1 (index 0 de l'échelle).
    let start_idx = 0usize;

    // 7. SB de fin : table finale à ~3 joueurs, M ≈ 5 BB → BB ≈ total/15.
    let end_sb_target = (total_chips / 30).max(bb1 * 2);
    let end_idx_min = find_ladder_index(end_sb_target, unit);
    let min_span_end = start_idx + (target_levels.saturating_sub(1) as usize);
    let end_idx = end_idx_min
        .max(min_span_end)
        .min(SB_LADDER_UNITS.len() - 1);

    let number_of_levels = target_levels.min((end_idx - start_idx + 1) as u32);

    // 8. Construction des niveaux.
    let ante_starts_at = (number_of_levels / 3).max(1);
    let break_after = if number_of_levels >= 6 {
        Some(number_of_levels / 2)
    } else {
        None
    };
    let break_minutes = break_duration(input.total_duration_minutes);

    let mut levels = Vec::with_capacity(number_of_levels as usize + 1);
    for i in 0..number_of_levels {
        let progress = if number_of_levels > 1 {
            i as f64 / (number_of_levels - 1) as f64
        } else {
            0.0
        };
        let idx_f = start_idx as f64 + (end_idx as f64 - start_idx as f64) * progress;
        let idx = (idx_f.round() as usize).min(SB_LADDER_UNITS.len() - 1);

        let sb = SB_LADDER_UNITS[idx] * unit;
        let bb = sb * 2;

        let ante = if i + 1 >= ante_starts_at {
            let raw = bb / 4;
            let rounded = ((raw as f64 / unit as f64).round() as u32) * unit;
            rounded.max(unit)
        } else {
            0
        };

        levels.push(BlindLevel {
            level: i + 1,
            small_blind: sb,
            big_blind: bb,
            ante,
            duration_minutes: level_duration_minutes,
            is_break: false,
        });

        if Some(i + 1) == break_after {
            levels.push(BlindLevel {
                level: 0,
                small_blind: 0,
                big_blind: 0,
                ante: 0,
                duration_minutes: break_minutes,
                is_break: true,
            });
        }
    }

    TournamentStructure {
        chips_per_player,
        starting_stack,
        total_chips,
        level_duration_minutes,
        number_of_levels,
        levels,
    }
}

/// Profondeur cible du stack en big blinds, dépendant de la durée et du nombre
/// de joueurs, **arrondie au multiple de 10** (100, 110, 120, …).
///
/// Formule : `duration_min / 3 + log2(players) * 10`, bornée à
/// \[[`MIN_DEPTH_BB`], [`MAX_DEPTH_BB`]\], puis arrondie au multiple de
/// [`DEPTH_STEP_BB`] le plus proche.
///
/// Justification :
/// - Base ∝ durée.
/// - Bonus ∝ log₂(N) : Harrington — un joueur doit doubler log₂(N) fois pour
///   gagner ; +10 BB par doublement compense la longueur structurelle requise.
/// - Arrondi à 10 BB pour avoir un chiffre rond (100/110/120…).
fn target_depth_bb(players: u32, duration_minutes: u32) -> u32 {
    let base = duration_minutes / 3;
    let log2_players = (players.max(2) as f64).log2();
    let bonus = (log2_players * 10.0).round() as u32;
    let raw = base
        .saturating_add(bonus)
        .clamp(MIN_DEPTH_BB, MAX_DEPTH_BB);
    let step = DEPTH_STEP_BB.max(1);
    // Arrondi au pas le plus proche, avec clamp pour ne pas sortir des bornes.
    let snapped = ((raw + step / 2) / step) * step;
    snapped.clamp(MIN_DEPTH_BB, MAX_DEPTH_BB)
}

/// Alloue le stack de départ à partir des N plus petites dénominations
/// disponibles dans la malette (N ≤ [`MAX_DENOMS_IN_STACK`]). Vise **exactement**
/// `depth_bb × bb1`. Si la malette ne permet pas cette cible ronde, décrémente
/// la profondeur par pas de [`DEPTH_STEP_BB`] jusqu'à trouver une cible
/// atteignable.
///
/// Privilégie les **petits jetons** : parmi toutes les compositions atteignant
/// la cible exacte, choisit celle qui maximise count(v1), puis count(v2), etc.
///
/// Garantit au moins **1 jeton de chaque dénomination** disponible (visibilité
/// ≥4 couleurs), et un minimum de [`MIN_V1_COUNT`] petits jetons quand
/// possible.
///
/// Retourne `(chips_per_player, starting_stack, bb1)` avec `bb1 = 2 × v1`.
fn allocate_stack(
    denoms: &[&ChipDenomination],
    players: u32,
    depth_bb: u32,
) -> (Vec<ChipDenomination>, u32, u32) {
    let players = players.max(1);
    if denoms.is_empty() {
        return (Vec::new(), 0, 1);
    }

    // Dénoms effectivement utilisables (count/players ≥ 1), limitées aux N plus petites.
    let mut usable: Vec<&ChipDenomination> = denoms
        .iter()
        .filter(|d| d.count / players >= 1)
        .copied()
        .collect();
    if usable.is_empty() {
        return (Vec::new(), 0, 2 * denoms[0].value);
    }
    usable.truncate(MAX_DENOMS_IN_STACK);

    let values: Vec<u32> = usable.iter().map(|d| d.value).collect();
    let available: Vec<u32> = usable.iter().map(|d| d.count / players).collect();

    let v1 = values[0];
    let bb1 = v1.saturating_mul(2);

    // Essaye la profondeur demandée, puis décrémente par pas de 10 BB jusqu'à
    // trouver une cible atteignable avec au moins 1 jeton de chaque dénom.
    // Va jusqu'à FLOOR_DEPTH_BB pour les malettes très contraintes.
    let mut depth = depth_bb.max(FLOOR_DEPTH_BB);
    loop {
        let target = depth.saturating_mul(bb1);
        if let Some(counts) = find_smooth_composition(&values, &available, target) {
            let chips: Vec<ChipDenomination> = values
                .iter()
                .zip(&counts)
                .filter(|(_, &c)| c > 0)
                .map(|(&v, &c)| ChipDenomination { value: v, count: c })
                .collect();
            let achieved: u32 =
                values.iter().zip(&counts).map(|(v, c)| v * c).sum();
            return (chips, achieved, bb1);
        }
        if depth <= FLOOR_DEPTH_BB {
            break;
        }
        depth = depth.saturating_sub(DEPTH_STEP_BB).max(FLOOR_DEPTH_BB);
    }

    // Fallback : aucune profondeur ronde ≥ FLOOR_DEPTH_BB n'est atteignable
    // exactement avec 1+ jetons de chaque dénom — malette très limitée.
    greedy_fallback(&values, &available, FLOOR_DEPTH_BB.saturating_mul(bb1), bb1)
}

/// Cherche une composition `counts[i]` telle que `Σ counts[i] × values[i] = target`,
/// avec `1 ≤ counts[i] ≤ available[i]` et contrainte de **décroissance** des
/// counts (`counts[0] ≥ counts[1] ≥ … ≥ counts[n-1]`) — plus de petits jetons
/// que de gros.
///
/// Parmi toutes les compositions valides, choisit celle qui **lisse** le mieux
/// la courbe des quantités : minimise le plus gros ratio entre deux comptages
/// consécutifs (`max_i counts[i] / counts[i+1]`). Tie-breaker : total de
/// jetons physiques plus petit.
///
/// Exemple pour target=200, malette 1/5/10/25 (dispo 30/30/10/5) :
/// - `(15, 10, 6, 3)` — ratio max = 2.0 ✓ (gagne)
/// - `(30, 21, 4, 1)` — ratio max = 5.25 (rejeté, courbe en escalier)
fn find_smooth_composition(
    values: &[u32],
    available: &[u32],
    target: u32,
) -> Option<Vec<u32>> {
    let n = values.len();
    if n == 0 {
        return if target == 0 { Some(Vec::new()) } else { None };
    }

    let v0 = values[0];
    let max0 = available[0];
    let min_v1 = MIN_V1_COUNT.min(max0).max(1);

    if n == 1 {
        if target % v0 == 0 {
            let c = target / v0;
            if c >= min_v1 && c <= max0 {
                return Some(vec![c]);
            }
        }
        return None;
    }

    let mut counts = vec![0u32; n];
    let mut best: Option<(Vec<u32>, CompScore)> = None;

    enumerate_smooth(
        n - 1,
        &mut counts,
        values,
        available,
        target,
        min_v1,
        &mut best,
    );

    best.map(|(c, _)| c)
}

/// Score de qualité d'une composition. Plus petit = meilleur. Ordre
/// lexicographique : (ratio max entre counts consécutifs ×1000, total jetons,
/// count[0]).
type CompScore = (u64, u32, u32);

fn compute_smoothness_score(counts: &[u32]) -> CompScore {
    let mut max_ratio_scaled = 1_000u64; // 1.0 × 1000
    for i in 0..counts.len().saturating_sub(1) {
        let (hi, lo) = (counts[i], counts[i + 1]);
        if lo > 0 && hi > 0 {
            let r = (hi as u64 * 1000) / lo as u64;
            if r > max_ratio_scaled {
                max_ratio_scaled = r;
            }
        }
    }
    let total: u32 = counts.iter().sum();
    let c0 = *counts.first().unwrap_or(&0);
    (max_ratio_scaled, total, c0)
}

/// Énumère récursivement toutes les compositions valides et retient la
/// meilleure selon [`compute_smoothness_score`].
///
/// Parcourt les counts **du dernier index (plus gros jeton, plus petit count)
/// vers le 1er** ; le count de v1 (idx 0) est dérivé par différence pour
/// garantir une somme exacte.
fn enumerate_smooth(
    idx: usize,
    counts: &mut Vec<u32>,
    values: &[u32],
    available: &[u32],
    target: u32,
    min_v1: u32,
    best: &mut Option<(Vec<u32>, CompScore)>,
) {
    let n = values.len();

    if idx == 0 {
        // Dérive count[0] par complément.
        let fixed: u32 = (1..n).map(|i| counts[i] * values[i]).sum();
        let Some(rem) = target.checked_sub(fixed) else {
            return;
        };
        if rem % values[0] != 0 {
            return;
        }
        let c0 = rem / values[0];
        // Doit respecter min_v1, availability, et c0 ≥ c1 (décroissance).
        let min_c0 = counts.get(1).copied().unwrap_or(1).max(min_v1);
        if c0 < min_c0 || c0 > available[0] {
            return;
        }
        counts[0] = c0;
        let score = compute_smoothness_score(counts);
        match best {
            None => *best = Some((counts.clone(), score)),
            Some((_, s)) if score < *s => *best = Some((counts.clone(), score)),
            _ => {}
        }
        return;
    }

    // Pour idx ≥ 1, itère de min_c à max_c.
    // min_c : ≥ counts[idx+1] (décroissance). Si idx == n-1, min_c = 1.
    // max_c : availability[idx].
    let lower_from_desc = if idx + 1 < n { counts[idx + 1] } else { 0 };
    let min_c = lower_from_desc.max(1);
    let max_c = available[idx];
    if min_c > max_c {
        return;
    }

    // Borne sup supplémentaire : ce qui peut rester pour v0..v_{idx-1}.
    // counts[idx] * values[idx] ≤ target − Σ_{j>idx} counts[j]*values[j].
    let already_fixed: u32 = ((idx + 1)..n).map(|i| counts[i] * values[i]).sum();
    let budget = target.saturating_sub(already_fixed);
    let cap_from_budget = budget / values[idx];
    let max_c = max_c.min(cap_from_budget);
    if min_c > max_c {
        return;
    }

    for c in min_c..=max_c {
        counts[idx] = c;
        enumerate_smooth(idx - 1, counts, values, available, target, min_v1, best);
    }
}

/// Fallback quand aucune cible ronde n'est atteignable : greedy best-effort
/// qui s'approche d'un stack minimal en préférant les petits jetons.
fn greedy_fallback(
    values: &[u32],
    available: &[u32],
    target: u32,
    bb1: u32,
) -> (Vec<ChipDenomination>, u32, u32) {
    let mut counts = vec![0u32; values.len()];
    for i in 0..values.len() {
        if available[i] >= 1 && values[i] <= target {
            counts[i] = 1;
        }
    }
    let mut achieved: u32 = values.iter().zip(&counts).map(|(v, c)| v * c).sum();

    // Remplit en priorité les plus petites, jusqu'à la cible.
    for i in 0..values.len() {
        if counts[i] >= available[i] || achieved >= target {
            continue;
        }
        let slack_global = (target - achieved) / values[i];
        let slack_avail = available[i] - counts[i];
        let add = slack_global.min(slack_avail);
        counts[i] += add;
        achieved += add * values[i];
    }

    let chips: Vec<ChipDenomination> = values
        .iter()
        .zip(&counts)
        .filter(|(_, &c)| c > 0)
        .map(|(&v, &c)| ChipDenomination { value: v, count: c })
        .collect();
    (chips, achieved, bb1)
}

/// Choisit la durée d'un niveau en visant ~6–12 niveaux selon la durée totale.
fn pick_level_duration(total_minutes: u32) -> u32 {
    match total_minutes {
        0..=5 => 1,
        6..=10 => 2,
        11..=20 => 3,
        21..=40 => 5,
        41..=80 => 8,
        81..=150 => 12,
        151..=300 => 20,
        301..=480 => 25,
        _ => 30,
    }
}

/// Durée de pause en fonction de la durée totale du tournoi.
fn break_duration(total_minutes: u32) -> u32 {
    match total_minutes {
        0..=29 => 0,
        30..=60 => 3,
        61..=120 => 5,
        121..=240 => 10,
        _ => 15,
    }
}

/// Trouve dans [`SB_LADDER_UNITS`] l'index dont la valeur × `unit` est la plus
/// proche de `target_sb`.
fn find_ladder_index(target_sb: u32, unit: u32) -> usize {
    let target_units = (target_sb as f64 / unit.max(1) as f64).max(1.0);
    SB_LADDER_UNITS
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let da = (**a as f64 - target_units).abs();
            let db = (**b as f64 - target_units).abs();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standard_malette_chips() -> Vec<ChipDenomination> {
        vec![
            ChipDenomination { value: 25, count: 100 },
            ChipDenomination { value: 100, count: 100 },
            ChipDenomination { value: 500, count: 50 },
            ChipDenomination { value: 1000, count: 25 },
        ]
    }

    fn server_malette_chips() -> Vec<ChipDenomination> {
        vec![
            ChipDenomination { value: 1, count: 150 },
            ChipDenomination { value: 5, count: 150 },
            ChipDenomination { value: 10, count: 50 },
            ChipDenomination { value: 25, count: 25 },
            ChipDenomination { value: 100, count: 50 },
            ChipDenomination { value: 500, count: 50 },
            ChipDenomination { value: 1000, count: 25 },
        ]
    }

    /// Malette typique 500 jetons pour 9 joueurs.
    fn standard_input() -> TournamentInput {
        TournamentInput {
            players: 9,
            total_duration_minutes: 240,
            case_chips: vec![
                ChipDenomination { value: 25, count: 100 },
                ChipDenomination { value: 100, count: 100 },
                ChipDenomination { value: 500, count: 50 },
                ChipDenomination { value: 1000, count: 25 },
            ],
        }
    }

    /// Malette serveur prod (williou), 5 joueurs.
    fn server_malette_5p(total_min: u32) -> TournamentInput {
        TournamentInput {
            players: 5,
            total_duration_minutes: total_min,
            case_chips: vec![
                ChipDenomination { value: 1, count: 150 },
                ChipDenomination { value: 5, count: 150 },
                ChipDenomination { value: 10, count: 50 },
                ChipDenomination { value: 25, count: 25 },
                ChipDenomination { value: 100, count: 50 },
                ChipDenomination { value: 500, count: 50 },
                ChipDenomination { value: 1000, count: 25 },
            ],
        }
    }

    #[test]
    fn uses_up_to_four_smallest_denominations() {
        // Malette standard a 4 dénominations — toutes doivent être utilisées.
        let s = compute_structure(&standard_input());
        assert!(
            (2..=MAX_DENOMS_IN_STACK).contains(&s.chips_per_player.len()),
            "stack doit utiliser 2 à 4 dénominations, a {}",
            s.chips_per_player.len()
        );
        let values: Vec<u32> = s.chips_per_player.iter().map(|c| c.value).collect();
        // Ce doit être un préfixe trié ascendant des dénominations.
        assert_eq!(values, vec![25, 100, 500, 1000]);
    }

    #[test]
    fn malette_with_more_than_four_denoms_uses_four_smallest() {
        // Malette serveur a 7 dénominations (1,5,10,25,100,500,1000) — on ne
        // doit en distribuer que les 4 plus petites.
        let s = compute_structure(&server_malette_5p(120));
        let values: Vec<u32> = s.chips_per_player.iter().map(|c| c.value).collect();
        assert_eq!(values, vec![1, 5, 10, 25]);
    }

    #[test]
    fn first_big_blind_equals_twice_smallest_chip() {
        let s = compute_structure(&standard_input());
        let first = s.levels.iter().find(|l| !l.is_break).unwrap();
        assert_eq!(first.small_blind, 25);
        assert_eq!(first.big_blind, 50);

        let s2 = compute_structure(&server_malette_5p(60));
        let first2 = s2.levels.iter().find(|l| !l.is_break).unwrap();
        assert_eq!(first2.small_blind, 1);
        assert_eq!(first2.big_blind, 2);
    }

    #[test]
    fn starting_stack_depth_is_within_dynamic_range() {
        let s = compute_structure(&standard_input());
        let bb1 = s.levels.iter().find(|l| !l.is_break).unwrap().big_blind;
        let depth = s.starting_stack as f64 / bb1 as f64;
        assert!(
            depth >= 20.0 && depth <= 160.0,
            "profondeur {} BB hors plage raisonnable",
            depth
        );
    }

    #[test]
    fn starting_stack_respects_malette_availability() {
        // 9 joueurs × (11×25 + 11×100 + 5×500 + 2×1000) = plafond 5875 par joueur.
        let s = compute_structure(&standard_input());
        assert!(
            s.starting_stack <= 5875,
            "stack {} > plafond malette 5875",
            s.starting_stack
        );
        for chip in &s.chips_per_player {
            match chip.value {
                25 => assert!(chip.count <= 11, "25: {}", chip.count),
                100 => assert!(chip.count <= 11, "100: {}", chip.count),
                500 => assert!(chip.count <= 5, "500: {}", chip.count),
                1000 => assert!(chip.count <= 2, "1000: {}", chip.count),
                v => panic!("dénomination inattendue {v}"),
            }
        }
    }

    #[test]
    fn stack_has_enough_small_chips_to_pay_blinds() {
        let s = compute_structure(&standard_input());
        let small = s.chips_per_player.iter().find(|c| c.value == 25).unwrap();
        assert!(small.count >= MIN_V1_COUNT.min(11));
    }

    #[test]
    fn blinds_are_monotonic() {
        let s = compute_structure(&standard_input());
        let playing: Vec<_> = s.levels.iter().filter(|l| !l.is_break).collect();
        for pair in playing.windows(2) {
            assert!(pair[1].big_blind >= pair[0].big_blind);
        }
    }

    #[test]
    fn break_is_inserted_for_long_tournaments() {
        let s = compute_structure(&standard_input());
        assert!(s.levels.iter().any(|l| l.is_break));
    }

    #[test]
    fn level_duration_is_picked_reasonably() {
        let s = compute_structure(&standard_input());
        assert_eq!(s.level_duration_minutes, 20);
    }

    #[test]
    fn falls_back_to_single_denomination_when_malette_too_thin() {
        let input = TournamentInput {
            players: 3,
            total_duration_minutes: 120,
            case_chips: vec![
                ChipDenomination { value: 25, count: 30 },
                ChipDenomination { value: 1000, count: 2 },
            ],
        };
        let s = compute_structure(&input);
        // 2/3 = 0 → v2 éliminée, reste v1 = 25 avec 30/3 = 10 dispo.
        assert_eq!(s.chips_per_player.len(), 1);
        assert_eq!(s.chips_per_player[0].value, 25);
        assert!(s.chips_per_player[0].count <= 10);
    }

    #[test]
    fn bb_is_always_double_sb() {
        let s = compute_structure(&server_malette_5p(60));
        for lvl in s.levels.iter().filter(|l| !l.is_break) {
            assert_eq!(lvl.big_blind, lvl.small_blind * 2);
        }
    }

    #[test]
    fn blinds_use_canonical_round_values() {
        let s = compute_structure(&server_malette_5p(60));
        let unit = 1;
        for lvl in s.levels.iter().filter(|l| !l.is_break) {
            let sb_in_units = lvl.small_blind / unit;
            assert!(
                SB_LADDER_UNITS.contains(&sb_in_units),
                "SB {} hors échelle",
                lvl.small_blind
            );
        }
    }

    #[test]
    fn five_minute_game_has_multiple_levels() {
        let s = compute_structure(&server_malette_5p(5));
        let playing = s.levels.iter().filter(|l| !l.is_break).count();
        assert!(playing >= 4);
        assert_eq!(s.level_duration_minutes, 1);
    }

    #[test]
    fn short_game_has_no_break() {
        let s = compute_structure(&server_malette_5p(10));
        assert!(!s.levels.iter().any(|l| l.is_break));
    }

    #[test]
    fn antes_kick_in_mid_tournament() {
        let s = compute_structure(&standard_input());
        let playing: Vec<_> = s.levels.iter().filter(|l| !l.is_break).collect();
        assert_eq!(playing.first().unwrap().ante, 0);
        assert!(playing.last().unwrap().ante > 0);
    }

    #[test]
    fn depth_formula_scales_with_players_and_duration() {
        assert!(target_depth_bb(16, 180) >= target_depth_bb(4, 180));
        assert!(target_depth_bb(8, 300) >= target_depth_bb(8, 60));
        assert!(target_depth_bb(2, 1) >= MIN_DEPTH_BB);
        assert!(target_depth_bb(100, 10_000) <= MAX_DEPTH_BB);
    }

    #[test]
    fn depth_is_always_a_round_number_of_bb() {
        for players in [2u32, 5, 8, 9, 16, 32] {
            for duration in [5u32, 30, 60, 120, 240, 480, 720] {
                let d = target_depth_bb(players, duration);
                assert_eq!(
                    d % DEPTH_STEP_BB,
                    0,
                    "profondeur {} BB pas ronde (players={}, dur={})",
                    d,
                    players,
                    duration
                );
                assert!(d >= MIN_DEPTH_BB && d <= MAX_DEPTH_BB);
            }
        }
    }

    #[test]
    fn starting_stack_hits_round_bb_when_malette_allows() {
        // Malette standard a largement de quoi fournir la profondeur cible.
        let s = compute_structure(&standard_input());
        let first = s.levels.iter().find(|l| !l.is_break).unwrap();
        let depth_bb = s.starting_stack / first.big_blind;
        assert_eq!(
            depth_bb * first.big_blind,
            s.starting_stack,
            "stack {} doit être un multiple entier de BB1={}",
            s.starting_stack,
            first.big_blind
        );
        assert_eq!(
            depth_bb % DEPTH_STEP_BB,
            0,
            "profondeur {} BB pas ronde",
            depth_bb
        );
    }

    #[test]
    fn stack_has_more_small_chips_than_big_ones() {
        // Avec malette standard, les poids favorisent v1 (plus petit) → plus de
        // petits que de gros (sauf saturation malette).
        let s = compute_structure(&standard_input());
        assert!(s.chips_per_player.len() >= 2);
        let smallest = s.chips_per_player.first().unwrap();
        let largest = s.chips_per_player.last().unwrap();
        assert!(
            smallest.count >= largest.count,
            "plus petit jeton ({}) = {} count, plus gros ({}) = {} count",
            smallest.value,
            smallest.count,
            largest.value,
            largest.count
        );
    }

    /// Aperçu : `cargo test preview_output -- --nocapture`.
    #[test]
    fn preview_output() {
        let scenarios: &[(u32, u32, &[ChipDenomination])] = &[
            (5, 5, &server_malette_chips()),
            (5, 60, &server_malette_chips()),
            (5, 120, &server_malette_chips()),
            (9, 240, &server_malette_chips()),
            (9, 240, &standard_malette_chips()),
        ];
        for (players, total, chips) in scenarios {
            let input = TournamentInput {
                players: *players,
                total_duration_minutes: *total,
                case_chips: chips.to_vec(),
            };
            let s = compute_structure(&input);
            eprintln!(
                "\n=== {} joueurs / {} min — stack={} total={} lvl={}'x{} chips={:?} ===",
                players,
                total,
                s.starting_stack,
                s.total_chips,
                s.level_duration_minutes,
                s.number_of_levels,
                s.chips_per_player
                    .iter()
                    .map(|c| format!("{}x{}", c.value, c.count))
                    .collect::<Vec<_>>()
            );
            for l in &s.levels {
                if l.is_break {
                    eprintln!("  -- BREAK {} min --", l.duration_minutes);
                } else {
                    eprintln!(
                        "  L{:>2}  SB {:>5}  BB {:>5}  ante {:>4}  ({} min)",
                        l.level, l.small_blind, l.big_blind, l.ante, l.duration_minutes
                    );
                }
            }
        }
    }
}
