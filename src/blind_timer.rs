use serde::{Deserialize, Serialize};

/// Paramètres d'entrée décrivant le tournoi que l'utilisateur veut organiser.
///
/// L'utilisateur décrit la **malette** (quantité totale de jetons par valeur)
/// et le nombre de joueurs. L'algorithme se charge de:
///   - répartir la malette équitablement entre les joueurs,
///   - choisir une durée de niveau adaptée,
///   - générer la structure de blinds correspondante.
#[derive(Debug, Deserialize, Clone)]
pub struct TournamentInput {
    /// Nombre de joueurs au départ.
    pub players: u32,
    /// Durée cible totale du tournoi, en minutes.
    pub total_duration_minutes: u32,
    /// Contenu total de la malette — valeur faciale => quantité dans la malette.
    /// Ex: {25: 100, 100: 100, 500: 50, 1000: 25}
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

/// Calcule la structure de blinds la plus adaptée aux paramètres fournis.
///
/// Étapes:
/// 1. Répartir la malette entre les joueurs (division entière par `players`).
/// 2. Calculer stack de départ par joueur et stack total en jeu.
/// 3. Choisir une durée de niveau visant ~12 niveaux sur la durée totale.
/// 4. BB initiale ~ stack/100 (≈100 BB de profondeur).
/// 5. BB finale ~ total/20 (phase push/fold).
/// 6. Progression géométrique entre ces deux bornes, arrondie au plus petit jeton.
/// 7. Antes à partir du 1/3 du tournoi, pause de 10 min au milieu.
pub fn compute_structure(input: &TournamentInput) -> TournamentStructure {
    // 1. Répartition de la malette entre les joueurs.
    let chips_per_player: Vec<ChipDenomination> = input
        .case_chips
        .iter()
        .map(|c| ChipDenomination {
            value: c.value,
            count: c.count / input.players.max(1),
        })
        .filter(|c| c.count > 0)
        .collect();

    // 2. Valeur du stack.
    let starting_stack: u32 = chips_per_player.iter().map(|c| c.value * c.count).sum();
    let total_chips = starting_stack * input.players;

    // 3. Durée de niveau auto-dérivée.
    let level_duration_minutes = pick_level_duration(input.total_duration_minutes);
    let number_of_levels = (input.total_duration_minutes / level_duration_minutes).max(1);

    // 4-5. Bornes de progression des blinds.
    let start_bb = (starting_stack / 100).max(2);
    let end_bb = (total_chips / 20).max(start_bb * 2);

    // 6. Plus petit jeton → unité d'arrondi pour les blinds.
    let smallest_chip = chips_per_player
        .iter()
        .map(|c| c.value)
        .min()
        .unwrap_or(25);

    let ratio = if number_of_levels > 1 {
        (end_bb as f64 / start_bb as f64).powf(1.0 / (number_of_levels - 1) as f64)
    } else {
        1.0
    };

    let mut levels = Vec::with_capacity(number_of_levels as usize + 1);
    let break_after_level = number_of_levels / 2;

    for i in 0..number_of_levels {
        let raw_bb = start_bb as f64 * ratio.powi(i as i32);
        let bb = round_to_nice(raw_bb as u32, smallest_chip);
        let sb = round_to_nice(bb / 2, smallest_chip).max(smallest_chip);
        let ante = if i >= number_of_levels / 3 {
            round_to_nice(bb / 10, smallest_chip)
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

        if i + 1 == break_after_level {
            levels.push(BlindLevel {
                level: 0,
                small_blind: 0,
                big_blind: 0,
                ante: 0,
                duration_minutes: 10,
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

/// Choisit une durée de niveau parmi {10, 15, 20, 25, 30} minutes
/// pour viser environ 12 niveaux sur la durée totale du tournoi.
fn pick_level_duration(total_minutes: u32) -> u32 {
    const TARGET_LEVELS: f64 = 12.0;
    const CANDIDATES: [u32; 5] = [10, 15, 20, 25, 30];
    let ideal = (total_minutes as f64 / TARGET_LEVELS).max(1.0);
    *CANDIDATES
        .iter()
        .min_by_key(|&&c| ((c as f64 - ideal).abs() * 100.0) as u32)
        .unwrap_or(&20)
}

/// Arrondit `value` au multiple de `step` le plus proche, en garantissant
/// au minimum un `step` (utile pour éviter les blinds à 0).
fn round_to_nice(value: u32, step: u32) -> u32 {
    if step == 0 {
        return value;
    }
    let rounded = ((value as f64 / step as f64).round() as u32) * step;
    rounded.max(step)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn chips_are_distributed_per_player() {
        let s = compute_structure(&standard_input());
        // 100/9=11, 100/9=11, 50/9=5, 25/9=2 → 11*25 + 11*100 + 5*500 + 2*1000 = 5875
        assert_eq!(s.starting_stack, 5875);
        assert_eq!(s.total_chips, 5875 * 9);
        assert_eq!(s.chips_per_player.len(), 4);
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
    fn break_is_inserted() {
        let s = compute_structure(&standard_input());
        assert!(s.levels.iter().any(|l| l.is_break));
    }

    #[test]
    fn level_duration_is_picked_reasonably() {
        // 240 min → ideal = 20 → doit tomber sur 20.
        let s = compute_structure(&standard_input());
        assert_eq!(s.level_duration_minutes, 20);
    }

    #[test]
    fn drops_denominations_with_zero_per_player() {
        // 3 joueurs, mais seulement 2 jetons de 1000 dans la malette → aucun 1000 distribué.
        let input = TournamentInput {
            players: 3,
            total_duration_minutes: 120,
            case_chips: vec![
                ChipDenomination { value: 25, count: 30 },
                ChipDenomination { value: 1000, count: 2 },
            ],
        };
        let s = compute_structure(&input);
        assert_eq!(s.chips_per_player.len(), 1);
        assert_eq!(s.chips_per_player[0].value, 25);
        assert_eq!(s.chips_per_player[0].count, 10);
    }
}
