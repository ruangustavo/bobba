#[derive(Clone, PartialEq, Eq)]
pub struct Rc4 {
    s: [u8; 256],
    i: u8,
    j: u8,
}

impl std::fmt::Debug for Rc4 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Rc4 {{ i: {}, j: {} }}", self.i, self.j)
    }
}

impl Rc4 {
    pub fn new(key: &[u8]) -> Option<Self> {
        if key.is_empty() {
            return None;
        }
        let mut s = [0u8; 256];
        for (slot, v) in s.iter_mut().zip(0u8..=255) {
            *slot = v;
        }
        let mut j = 0u8;
        for i in 0..256 {
            j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
            s.swap(i, usize::from(j));
        }
        Some(Self { s, i: 0, j: 0 })
    }

    pub fn from_state(s: [u8; 256], i: u8, j: u8) -> Self {
        Self { s, i, j }
    }

    pub fn state(&self) -> &[u8; 256] {
        &self.s
    }

    pub fn i(&self) -> u8 {
        self.i
    }

    pub fn j(&self) -> u8 {
        self.j
    }

    pub fn could_be_fresh(&self) -> bool {
        self.i == 0 && self.j == 0
    }

    pub fn apply(&mut self, data: &mut [u8]) {
        for byte in data {
            self.i = self.i.wrapping_add(1);
            self.j = self.j.wrapping_add(self.s[usize::from(self.i)]);
            self.s.swap(usize::from(self.i), usize::from(self.j));
            let k = self.s[usize::from(
                self.s[usize::from(self.i)].wrapping_add(self.s[usize::from(self.j)]),
            )];
            *byte ^= k;
        }
    }

    pub fn rewind(&mut self, n: usize) {
        for _ in 0..n {
            self.s.swap(usize::from(self.i), usize::from(self.j));
            self.j = self.j.wrapping_sub(self.s[usize::from(self.i)]);
            self.i = self.i.wrapping_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_rfc6229_test_vector() {
        let mut rc4 = Rc4::new(&[1, 2, 3, 4, 5]).expect("non-empty key");
        let mut stream = [0u8; 16];
        rc4.apply(&mut stream);
        assert_eq!(
            stream,
            [
                0xb2, 0x39, 0x63, 0x05, 0xf0, 0x3d, 0xc0, 0x27, 0xcc, 0xc3, 0x52, 0x4a, 0x0a, 0x11,
                0x18, 0xa8
            ]
        );
    }

    #[test]
    fn rewind_restores_the_exact_starting_state() {
        let fresh = Rc4::new(b"bobba").expect("non-empty key");
        let mut rc4 = fresh.clone();
        let mut data = vec![7u8; 1000];
        rc4.apply(&mut data);
        rc4.rewind(1000);
        assert_eq!(rc4, fresh);
        assert!(rc4.could_be_fresh());
    }

    #[test]
    fn rejects_an_empty_key() {
        assert!(Rc4::new(&[]).is_none());
    }
}
