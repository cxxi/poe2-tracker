//! Lecture des tables du jeu (.datc64) : un nombre de lignes, des lignes de largeur fixe, puis une
//! zone de données (chaînes en UTF-16, listes) repérée par huit octets 0xBB. Les colonnes n'ont
//! pas de nom dans le fichier : on lit à des positions connues, vérifiées à l'usage.

pub struct Dat {
    data: Vec<u8>,
    pub rows: usize,
    width: usize,
    /// Début de la zone de données (les décalages des chaînes et listes partent de là).
    heap: usize,
}

const HEAP_MARK: [u8; 8] = [0xbb; 8];

impl Dat {
    pub fn new(data: Vec<u8>) -> Option<Dat> {
        let rows = u32::from_le_bytes(data.get(0..4)?.try_into().ok()?) as usize;
        let heap = data.windows(8).position(|w| w == HEAP_MARK)?;
        let width = if rows == 0 { 0 } else { (heap - 4) / rows };
        Some(Dat { data, rows, width, heap })
    }

    fn at(&self, row: usize, col: usize, len: usize) -> Option<&[u8]> {
        if row >= self.rows || col + len > self.width {
            return None;
        }
        let p = 4 + row * self.width + col;
        self.data.get(p..p + len)
    }

    pub fn u8(&self, row: usize, col: usize) -> u8 {
        self.at(row, col, 1).map_or(0, |b| b[0])
    }

    pub fn u16(&self, row: usize, col: usize) -> u16 {
        self.at(row, col, 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn i32(&self, row: usize, col: usize) -> i32 {
        self.at(row, col, 4).map_or(0, |b| i32::from_le_bytes(b.try_into().unwrap()))
    }

    pub fn u64(&self, row: usize, col: usize) -> u64 {
        self.at(row, col, 8).map_or(0, |b| u64::from_le_bytes(b.try_into().unwrap()))
    }

    /// Chaîne UTF-16 de la zone de données, terminée par deux octets nuls.
    pub fn string(&self, row: usize, col: usize) -> String {
        let start = self.heap + self.u64(row, col) as usize;
        let mut units = Vec::new();
        let mut p = start;
        while let Some(b) = self.data.get(p..p + 2) {
            let u = u16::from_le_bytes([b[0], b[1]]);
            if u == 0 {
                break;
            }
            units.push(u);
            p += 2;
        }
        String::from_utf16_lossy(&units)
    }

    /// Liste de lignes d'une autre table (16 octets par élément : numéro de ligne, puis rien).
    pub fn row_list(&self, row: usize, col: usize) -> Vec<usize> {
        let (count, offset) = (self.u64(row, col) as usize, self.u64(row, col + 8) as usize);
        (0..count.min(256))
            .filter_map(|k| {
                let p = self.heap + offset + k * 16;
                Some(u64::from_le_bytes(self.data.get(p..p + 8)?.try_into().ok()?) as usize)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_strings_and_lists() {
        // Deux lignes de 24 octets : chaîne (8), liste (16).
        let mut data = 2u32.to_le_bytes().to_vec();
        let heap_rows = |s: u64, n: u64, o: u64| [s.to_le_bytes(), n.to_le_bytes(), o.to_le_bytes()].concat();
        data.extend(heap_rows(8, 1, 14));
        data.extend(heap_rows(14, 0, 0));
        data.extend(HEAP_MARK);
        data.extend("ab\0".encode_utf16().flat_map(|u| u.to_le_bytes())); // décalage 8
        data.extend([0u8; 0]);
        data.extend(7u64.to_le_bytes()); // décalage 14 : liste d'un élément → ligne 7
        data.extend(0u64.to_le_bytes());
        let d = Dat::new(data).unwrap();
        assert_eq!((d.rows, d.string(0, 0)), (2, "ab".to_string()));
        assert_eq!(d.row_list(0, 8), vec![7]);
    }
}
