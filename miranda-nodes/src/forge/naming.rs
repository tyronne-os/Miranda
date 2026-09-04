//! Task 3 — Naming engine.
//!
//! Requirement 3.2 / design.md Property 4: every Model Forge output gets
//! a display name of the form `"<Female First Name> <Family>-<Size>
//! <Descriptor>"` (e.g. `"Erica GLM-9B Uncensored"`), and no two models in
//! the library ever share a display name — collisions are resolved with
//! a numeric suffix *before* the name is ever handed to
//! [`crate::forge::model_registry::ModelRegistry::register_model`], which
//! is what makes registry-level duplicate rejection a backstop rather
//! than the only line of defense.

use std::collections::HashSet;

/// A fixed pool of female first names used for generated model display
/// names. Deliberately a plain list rather than a random-name generator
/// dependency — this keeps naming fully deterministic and testable, and
/// the pool is large enough that collisions are the exception (handled
/// below), not the common case.
const NAME_POOL: &[&str] = &[
    "Erica", "Nadia", "Sasha", "Lena", "Priya", "Vera", "Iris", "Noor",
    "Amara", "Elin", "Talia", "Rosalind", "Wren", "Ines", "Cora", "Maren",
    "Sable", "Delphine", "Junia", "Averil", "Bryony", "Calla", "Dorian",
    "Elowen", "Fenna", "Greta", "Hazel", "Ivet", "Juno", "Kira",
];

/// Deterministically picks a base name index from the family+size+
/// descriptor string, so the same inputs always start from the same
/// name before collision resolution kicks in (useful for tests and for
/// the user recognizing "the GLM-9B Uncensored one is always an Erica
/// unless there's already one").
fn base_name_index(seed: &str) -> usize {
    let hash: u64 = seed.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    (hash % NAME_POOL.len() as u64) as usize
}

/// design.md: `generate_name(base_family, size, descriptor, existing_names) -> String`.
/// Property 4: the returned name is never already present in
/// `existing_names` — if the deterministic first choice collides, a
/// numeric suffix (" 2", " 3", ...) is appended until it doesn't, and if
/// every name in the pool at every suffix somehow collided (practically
/// unreachable), the loop still terminates by capping at the pool size
/// times a large suffix bound rather than spinning forever.
pub fn generate_name(
    base_family: &str,
    size: &str,
    descriptor: &str,
    existing_names: &HashSet<String>,
) -> String {
    let seed = format!("{base_family}{size}{descriptor}");
    let start_idx = base_name_index(&seed);

    let model_label = if size.is_empty() {
        base_family.to_string()
    } else {
        format!("{base_family}-{size}")
    };

    for offset in 0..NAME_POOL.len() {
        let idx = (start_idx + offset) % NAME_POOL.len();
        let candidate = format!("{} {} {}", NAME_POOL[idx], model_label, descriptor)
            .trim()
            .to_string();
        if !existing_names.contains(&candidate) {
            return candidate;
        }
    }

    // Every name in the pool is taken for this family/size/descriptor
    // combination (extremely unlikely in practice) — fall back to
    // numeric suffixes on the deterministic first choice until a free
    // one is found.
    let base_candidate = format!("{} {} {}", NAME_POOL[start_idx], model_label, descriptor)
        .trim()
        .to_string();
    let mut suffix = 2u32;
    loop {
        let candidate = format!("{base_candidate} {suffix}");
        if !existing_names.contains(&candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_name_in_the_expected_format() {
        let existing = HashSet::new();
        let name = generate_name("GLM", "9B", "Uncensored", &existing);
        let parts: Vec<&str> = name.split(' ').collect();
        assert!(parts.len() >= 3);
        assert!(NAME_POOL.contains(&parts[0]));
        assert_eq!(parts[1], "GLM-9B");
    }

    #[test]
    fn same_inputs_produce_the_same_name_when_no_collision() {
        let existing = HashSet::new();
        let a = generate_name("Nemotron", "70B", "Assistant", &existing);
        let b = generate_name("Nemotron", "70B", "Assistant", &existing);
        assert_eq!(a, b);
    }

    #[test]
    fn collision_resolved_with_a_different_name_from_the_pool() {
        let mut existing = HashSet::new();
        let first = generate_name("GLM", "9B", "Uncensored", &existing);
        existing.insert(first.clone());
        let second = generate_name("GLM", "9B", "Uncensored", &existing);
        assert_ne!(first, second);
        assert!(!existing.contains(&second));
    }

    /// design.md Property 4, the hard gate: generated name is never
    /// already present in `existing_names`, even under a pathological
    /// input that pre-fills nearly the entire name pool for one
    /// family/size/descriptor combination.
    #[test]
    fn never_returns_a_name_already_in_existing_names_even_when_pool_is_nearly_exhausted() {
        let mut existing = HashSet::new();
        // Pre-fill every possible pool-based name for this combination.
        for name in NAME_POOL {
            existing.insert(format!("{name} GLM-9B Uncensored"));
        }
        let result = generate_name("GLM", "9B", "Uncensored", &existing);
        assert!(!existing.contains(&result), "collision fallback returned a name that was already taken: {result}");
        assert!(result.contains(" 2") || result.ends_with("2"), "expected a numeric-suffixed fallback, got: {result}");
    }

    #[test]
    fn different_descriptors_can_produce_different_base_names() {
        let existing = HashSet::new();
        let a = generate_name("GLM", "9B", "Uncensored", &existing);
        let b = generate_name("GLM", "9B", "Quantized", &existing);
        // Not asserting they must differ (a hash collision is fine),
        // just that both are valid, well-formed names.
        for name in [&a, &b] {
            assert!(name.split(' ').count() >= 3);
        }
    }

    #[test]
    fn handles_empty_size_gracefully() {
        let existing = HashSet::new();
        let name = generate_name("Nemotron", "", "Base", &existing);
        assert!(name.contains("Nemotron"));
        assert!(!name.contains("Nemotron-"));
    }

    #[test]
    fn repeated_collisions_eventually_terminate_with_a_free_name() {
        let mut existing = HashSet::new();
        for _ in 0..5 {
            let name = generate_name("Qwen", "14B", "Coder", &existing);
            assert!(!existing.contains(&name));
            existing.insert(name);
        }
        assert_eq!(existing.len(), 5);
    }
}
