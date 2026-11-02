//! Robustesse : un fichier en cours d'écriture (tronqué) ou corrompu ne doit
//! jamais faire paniquer le parseur.

mod common;

use gsf_core::sv::parse;

/// Générateur pseudo-aléatoire déterministe (xorshift64*), sans dépendance.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn truncated_fixtures_are_errors_not_panics() {
    for (name, bytes) in common::fixtures() {
        // Coupé au milieu d'une table : forcément invalide.
        let cut = bytes.len() / 2;
        assert!(
            parse(&bytes[..cut]).is_err(),
            "{name}: tronqué à {cut} accepté"
        );
        // Dernière accolade retirée.
        let end = bytes.iter().rposition(|&b| b == b'}').unwrap();
        assert!(
            parse(&bytes[..end]).is_err(),
            "{name}: sans `}}` final accepté"
        );
    }
}

#[test]
fn random_truncations_never_panic() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for (_, bytes) in common::fixtures() {
        for _ in 0..300 {
            let cut = rng.below(bytes.len() + 1);
            let _ = parse(&bytes[..cut]);
        }
        // Toutes les coupes des 2 000 premiers octets (zone la plus variée).
        for cut in 0..bytes.len().min(2_000) {
            let _ = parse(&bytes[..cut]);
        }
    }
}

#[test]
fn random_byte_corruption_never_panics() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let specials = b"{}[]=,;-\"'\\\r\n0123456789.eE[[]]-- nil\xFF\x00";
    for (_, bytes) in common::fixtures() {
        for _ in 0..200 {
            let mut b = bytes.clone();
            for _ in 0..1 + rng.below(4) {
                let i = rng.below(b.len());
                b[i] = specials[rng.below(specials.len())];
            }
            let _ = parse(&b);
        }
    }
}

#[test]
fn arbitrary_bytes_never_panic() {
    let mut rng = Rng(42);
    for len in 0..512 {
        let b: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        let _ = parse(&b);
    }
}
