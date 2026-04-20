use serde::{Deserialize, Serialize};

/// Paramètres d'entrée décrivant le tournoi que l'utilisateur veut organiser.
#[derive(Debug, Deserialize, Clone)]
pub struct TournamentInput {
    /// Nombre de joueurs au départ.
    pub players: u32,
    /// Durée cible totale du tournoi, en minutes.
    pub target_duration_minutes: u32,
    /// Durée d'un niveau de blind, en minutes (classique: 15 ou 20).
    pub level_duration_minutes: u32,
    /// Dotation en jetons par joueur — valeur faciale du jeton => quantité.
    /// Ex: {25: 8, 100: 10, 500: 4, 1000: 2}
    pub chips_per_player: Vec<ChipDenomination>,
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
    pub starting_stack: u32,
    pub total_chips: u32,
    pub number_of_levels: u32,
    pub levels: Vec<BlindLevel>,
}

/// Calcule la structure de blinds la plus adaptée aux paramètres fournis.
///
/// Algorithme:
/// 1. On calcule le stack de départ par joueur et le stack total en jeu.
/// 2. Le nombre de niveaux = durée totale / durée par niveau.
/// 3. La petite blind initiale vise ~100 BB de profondeur (confort en début).
/// 4. La grosse blind finale vise ~5% du stack total (force les all-in en fin).
/// 5. Les niveaux suivent une progression géométrique entre ces deux bornes,
///    puis chaque valeur est arrondie à un multiple "propre" basé sur les
///    dénominations de jetons disponibles.
/// 6. Une pause de 10 min est insérée au milieu du tournoi.
pub fn compute_structure(input: &TournamentInput) -> TournamentStructure {
    let starting_stack: u32 = input
        .chips_per_player
        .iter()
        .map(|c| c.value * c.count)
        .sum();
    let total_chips = starting_stack * input.players;

    let number_of_levels = (input.target_duration_minutes / input.level_duration_minutes).max(1);

    // Règles empiriques:
    //   BB initiale ≈ stack / 100  → 100 BB de profondeur au départ.
    //   BB finale   ≈ total / 20   → 5% du stack total (phase push/fold).
    let start_bb = (starting_stack / 100).max(2);
    let end_bb = (total_chips / 20).max(start_bb * 2);

    // Plus petit jeton disponible → sert d'unité d'arrondi pour les blinds.
    let smallest_chip = input
        .chips_per_player
        .iter()
        .map(|c| c.value)
        .min()
        .unwrap_or(25);

    // Progression géométrique: bb(n) = start_bb * r^n avec r choisi
    // pour que bb(N-1) = end_bb.
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
        // Ante à partir du 1/3 du tournoi, ~10% de la BB.
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
            duration_minutes: input.level_duration_minutes,
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
        starting_stack,
        total_chips,
        number_of_levels,
        levels,
    }
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

    fn standard_input() -> TournamentInput {
        TournamentInput {
            players: 9,
            target_duration_minutes: 240,
            level_duration_minutes: 20,
            chips_per_player: vec![
                ChipDenomination { value: 25, count: 8 },
                ChipDenomination { value: 100, count: 10 },
                ChipDenomination { value: 500, count: 4 },
                ChipDenomination { value: 1000, count: 2 },
            ],
        }
    }

    #[test]
    fn total_chips_matches_player_count() {
        let s = compute_structure(&standard_input());
        assert_eq!(s.starting_stack, 5200);
        assert_eq!(s.total_chips, 5200 * 9);
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
}
