//! Lecture des archives du jeu (Bundles2) pour en extraire des images d'interface.
//!
//! Chez Steam, Bundles2 est un dossier ; la version autonome range le même contenu dans
//! `Content.ggpk` (enregistrements PDIR = dossier, FILE = fichier).
//!
//! Format documenté par la communauté (LibBundle / LibGGPK3) :
//! - un « bundle » est un en-tête suivi de blocs compressés avec Oodle ;
//! - `_.index.bin` est lui-même un bundle : liste des bundles, des fichiers (hash du chemin,
//!   bundle, position, taille), des dossiers, puis un bundle imbriqué qui permet de
//!   reconstruire les chemins.
//!
//! Lecture seule : on ne modifie jamais les fichiers du jeu.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

type Res<T> = Result<T, Box<dyn std::error::Error>>;

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }
    fn bytes(&mut self, n: usize) -> Res<&'a [u8]> {
        let s = self.data.get(self.pos..self.pos + n).ok_or(tr!("fin de données inattendue"))?;
        self.pos += n;
        Ok(s)
    }
    fn u32(&mut self) -> Res<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into()?))
    }
    fn u64(&mut self) -> Res<u64> {
        Ok(u64::from_le_bytes(self.bytes(8)?.try_into()?))
    }
    fn rest(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }
}

/// Décompresse un bundle complet (en-tête + blocs Oodle).
pub fn decompress_bundle(raw: &[u8]) -> Res<Vec<u8>> {
    let mut r = Reader::new(raw);
    let _uncompressed_size = r.u32()?;
    let _compressed_size = r.u32()?;
    let _head_size = r.u32()?;
    let _compressor = r.u32()?;
    let _unknown = r.u32()?;
    let uncompressed_size = r.u64()? as usize;
    let _compressed_size = r.u64()?;
    let block_count = r.u32()? as usize;
    let granularity = r.u32()? as usize;
    r.bytes(16)?; // 4 entiers inconnus
    let sizes: Vec<usize> = (0..block_count).map(|_| r.u32().map(|v| v as usize)).collect::<Res<_>>()?;

    let mut out = vec![0u8; uncompressed_size];
    let mut extractor = oozextract::Extractor::new();
    for (i, size) in sizes.iter().enumerate() {
        let block = r.bytes(*size)?;
        let start = i * granularity;
        let end = (start + granularity).min(uncompressed_size);
        extractor
            .read_from_slice(block, &mut out[start..end])
            .map_err(|e| format!("bloc {i} : {e:?}"))?;
    }
    Ok(out)
}

/// MurmurHash64A, utilisé par le jeu pour indexer les chemins.
pub fn murmur64a(key: &[u8], seed: u64) -> u64 {
    const M: u64 = 0xc6a4a7935bd1e995;
    const R: u32 = 47;
    let mut h = seed ^ (key.len() as u64).wrapping_mul(M);
    let chunks = key.chunks_exact(8);
    let tail = chunks.remainder();
    for c in chunks {
        let mut k = u64::from_le_bytes(c.try_into().unwrap());
        k = k.wrapping_mul(M);
        k ^= k >> R;
        k = k.wrapping_mul(M);
        h ^= k;
        h = h.wrapping_mul(M);
    }
    if !tail.is_empty() {
        for (i, b) in tail.iter().enumerate() {
            h ^= (*b as u64) << (8 * i);
        }
        h = h.wrapping_mul(M);
    }
    h ^= h >> R;
    h = h.wrapping_mul(M);
    h ^= h >> R;
    h
}

const PATH_SEED: u64 = 0x1337b33f;

pub fn path_hash(path: &str) -> u64 {
    murmur64a(path.to_lowercase().as_bytes(), PATH_SEED)
}

#[derive(Clone, Copy)]
pub struct FileEntry {
    pub bundle: u32,
    pub offset: u32,
    pub size: u32,
}

/// Où lire les fichiers de Bundles2.
enum Source {
    Dir(PathBuf),
    /// Archive et, pour chaque chemin sous Bundles2 (minuscules) : position et taille des données.
    Ggpk(PathBuf, HashMap<String, (u64, u64)>),
}

impl Source {
    fn open(game_dir: &Path) -> Res<Source> {
        let dir = game_dir.join("Bundles2");
        if dir.join("_.index.bin").is_file() {
            return Ok(Source::Dir(dir));
        }
        let ggpk = game_dir.join("Content.ggpk");
        if ggpk.is_file() {
            let files = ggpk_bundles(&ggpk)?;
            return Ok(Source::Ggpk(ggpk, files));
        }
        Err(tr!("ni Bundles2 ni Content.ggpk dans {dir}", dir = game_dir.display()).into())
    }

    fn read(&self, name: &str) -> Res<Vec<u8>> {
        match self {
            Source::Dir(dir) => {
                let mut raw = Vec::new();
                File::open(dir.join(name))?.read_to_end(&mut raw)?;
                Ok(raw)
            }
            Source::Ggpk(path, files) => {
                let (offset, len) = *files.get(&name.to_lowercase()).ok_or_else(|| tr!("absent de Content.ggpk : Bundles2/{name}", name))?;
                let mut f = File::open(path)?;
                f.seek(SeekFrom::Start(offset))?;
                let mut raw = vec![0u8; len as usize];
                f.read_exact(&mut raw)?;
                Ok(raw)
            }
        }
    }
}

fn read_at(f: &mut File, offset: u64, n: usize) -> Res<Vec<u8>> {
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; n];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

/// Nom d'un enregistrement : UTF-16 (version 3) ou UTF-32 (version 4), terminé par un zéro.
fn ggpk_name(bytes: &[u8], wide: bool) -> String {
    if wide {
        bytes.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).take_while(|c| *c != 0).filter_map(char::from_u32).collect()
    } else {
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|c| *c != 0).collect();
        String::from_utf16_lossy(&units)
    }
}

enum GgpkRecord {
    Dir { name: String, children: Vec<u64> },
    File { name: String, data: u64, len: u64 },
}

fn ggpk_record(f: &mut File, offset: u64, wide: bool) -> Res<GgpkRecord> {
    let head = read_at(f, offset, 12)?;
    let len = u32::from_le_bytes(head[0..4].try_into()?) as u64;
    let tag = &head[4..8];
    let name_len = u32::from_le_bytes(head[8..12].try_into()?) as usize * if wide { 4 } else { 2 };
    match tag {
        b"PDIR" => {
            let rest = read_at(f, offset + 12, 4 + 32 + name_len)?;
            let count = u32::from_le_bytes(rest[0..4].try_into()?) as usize;
            let name = ggpk_name(&rest[36..], wide);
            let entries = read_at(f, offset + 12 + 4 + 32 + name_len as u64, count * 12)?;
            let children = entries.chunks_exact(12).map(|e| u64::from_le_bytes(e[4..12].try_into().unwrap())).collect();
            Ok(GgpkRecord::Dir { name, children })
        }
        b"FILE" => {
            let name = ggpk_name(&read_at(f, offset + 12 + 32, name_len)?, wide);
            let header = 12 + 32 + name_len as u64;
            Ok(GgpkRecord::File { name, data: offset + header, len: len.saturating_sub(header) })
        }
        other => Err(format!("enregistrement GGPK inattendu {:?}", String::from_utf8_lossy(other)).into()),
    }
}

/// Inventaire de Bundles2 dans Content.ggpk.
fn ggpk_bundles(path: &Path) -> Res<HashMap<String, (u64, u64)>> {
    let mut f = File::open(path)?;
    let head = read_at(&mut f, 0, 28)?;
    if &head[4..8] != b"GGPK" {
        return Err(tr!("Content.ggpk : en-tête GGPK absent").into());
    }
    let version = u32::from_le_bytes(head[8..12].try_into()?);
    let wide = version >= 4;
    // Deux positions : la racine (PDIR) et la liste des espaces libres (FREE), dans un ordre variable.
    let offsets = [u64::from_le_bytes(head[12..20].try_into()?), u64::from_le_bytes(head[20..28].try_into()?)];
    let children = offsets
        .iter()
        .find_map(|&o| match ggpk_record(&mut f, o, wide) {
            Ok(GgpkRecord::Dir { children, .. }) => Some(children),
            _ => None,
        })
        .ok_or(tr!("Content.ggpk : racine introuvable"))?;
    let bundles = children
        .into_iter()
        .find(|&c| matches!(ggpk_record(&mut f, c, wide), Ok(GgpkRecord::Dir { ref name, .. }) if name.eq_ignore_ascii_case("Bundles2")))
        .ok_or(tr!("Content.ggpk : pas de dossier Bundles2"))?;
    let mut files = HashMap::new();
    let mut stack = vec![(bundles, String::new())];
    while let Some((offset, prefix)) = stack.pop() {
        match ggpk_record(&mut f, offset, wide)? {
            GgpkRecord::Dir { name, children } => {
                // La racine de Bundles2 ne préfixe pas ses enfants.
                let path = if offset == bundles { String::new() } else { format!("{prefix}{}/", name.to_lowercase()) };
                stack.extend(children.into_iter().map(|c| (c, path.clone())));
            }
            GgpkRecord::File { name, data, len } => {
                files.insert(format!("{prefix}{}", name.to_lowercase()), (data, len));
            }
        }
    }
    Ok(files)
}

pub struct Index {
    source: Source,
    bundles: Vec<String>,
    files: HashMap<u64, FileEntry>,
    pub paths: Vec<String>,
}

impl Index {
    /// `game_dir` = dossier d'installation du jeu (Steam ou version autonome).
    pub fn open(game_dir: &Path) -> Res<Index> {
        let source = Source::open(game_dir)?;
        let data = decompress_bundle(&source.read("_.index.bin")?)?;
        let mut r = Reader::new(&data);

        let bundle_count = r.u32()? as usize;
        let mut bundles = Vec::with_capacity(bundle_count);
        for _ in 0..bundle_count {
            let len = r.u32()? as usize;
            bundles.push(String::from_utf8_lossy(r.bytes(len)?).into_owned());
            r.u32()?; // taille décompressée
        }

        let file_count = r.u32()? as usize;
        let mut files = HashMap::with_capacity(file_count);
        for _ in 0..file_count {
            let hash = r.u64()?;
            let entry = FileEntry { bundle: r.u32()?, offset: r.u32()?, size: r.u32()? };
            files.insert(hash, entry);
        }

        let dir_count = r.u32()? as usize;
        let mut dirs = Vec::with_capacity(dir_count);
        for _ in 0..dir_count {
            let _hash = r.u64()?;
            let offset = r.u32()? as usize;
            let size = r.u32()? as usize;
            let _recursive = r.u32()?;
            dirs.push((offset, size));
        }

        let path_data = decompress_bundle(r.rest())?;
        let mut paths = Vec::with_capacity(file_count);
        for (offset, size) in dirs {
            read_paths(&path_data[offset..offset + size], &mut paths)?;
        }
        Ok(Index { source, bundles, files, paths })
    }

    pub fn entry(&self, path: &str) -> Option<FileEntry> {
        self.files.get(&path_hash(path)).copied()
    }

    /// Part des chemins reconstruits qui correspondent à un fichier de l'index (contrôle du hash).
    pub fn hash_check(&self, sample: usize) -> (usize, usize) {
        let n = self.paths.len().min(sample);
        let hits = self.paths.iter().take(n).filter(|p| self.entry(p).is_some()).count();
        (hits, n)
    }

    /// Comme `read`, en gardant les derniers bundles décompressés (plusieurs fichiers d'un même bundle).
    pub fn read_cached(&self, path: &str, cache: &mut Vec<(u32, Vec<u8>)>) -> Res<Vec<u8>> {
        let e = self.entry(path).ok_or_else(|| tr!("fichier absent de l'index : {path}", path))?;
        if let Some(i) = cache.iter().position(|(b, _)| *b == e.bundle) {
            let (_, data) = &cache[i];
            return Ok(data[e.offset as usize..(e.offset + e.size) as usize].to_vec());
        }
        let name = &self.bundles[e.bundle as usize];
        let data = decompress_bundle(&self.source.read(&format!("{name}.bundle.bin"))?)?;
        let out = data[e.offset as usize..(e.offset + e.size) as usize].to_vec();
        cache.push((e.bundle, data));
        if cache.len() > 6 {
            cache.remove(0);
        }
        Ok(out)
    }

    pub fn read(&self, path: &str) -> Res<Vec<u8>> {
        let e = self.entry(path).ok_or_else(|| tr!("fichier absent de l'index : {path}", path))?;
        let name = &self.bundles[e.bundle as usize];
        let data = decompress_bundle(&self.source.read(&format!("{name}.bundle.bin"))?)?;
        Ok(data[e.offset as usize..(e.offset + e.size) as usize].to_vec())
    }
}

/// Reconstitue les chemins d'un dossier : alternance de « bases » (préfixes) et de chemins.
fn read_paths(data: &[u8], out: &mut Vec<String>) -> Res<()> {
    let mut r = Reader::new(data);
    let mut bases: Vec<String> = Vec::new();
    let mut base_mode = false;
    while r.pos + 4 <= data.len() {
        let index = r.u32()? as usize;
        if index == 0 {
            base_mode = !base_mode;
            if base_mode {
                bases.clear();
            }
            continue;
        }
        let start = r.pos;
        let end = data[start..].iter().position(|b| *b == 0).map(|i| start + i).ok_or(tr!("chaîne non terminée"))?;
        let mut s = String::from_utf8_lossy(&data[start..end]).into_owned();
        r.pos = end + 1;
        if let Some(prefix) = bases.get(index - 1) {
            s = format!("{prefix}{s}");
        }
        if base_mode {
            bases.push(s);
        } else {
            out.push(s);
        }
    }
    Ok(())
}

/// Décode une texture DDS (BC1, BC3, BC7 ou RGBA non compressé) en image RGBA.
pub fn decode_dds(data: &[u8]) -> Res<image::RgbaImage> {
    let mut r = Reader::new(data);
    if r.bytes(4)? != b"DDS " {
        return Err(tr!("pas un fichier DDS").into());
    }
    let header = r.bytes(124)?;
    let u32_at = |o: usize| u32::from_le_bytes(header[o..o + 4].try_into().unwrap());
    let height = u32_at(8) as usize;
    let width = u32_at(12) as usize;
    let four_cc = &header[80..84];
    let (format, block_bytes) = match four_cc {
        b"DXT1" => ("bc1", 8),
        b"DXT5" => ("bc3", 16),
        b"DX10" => {
            let dxgi = r.u32()?;
            r.bytes(16)?;
            match dxgi {
                71 | 72 => ("bc1", 8),
                77 | 78 => ("bc3", 16),
                98 | 99 => ("bc7", 16),
                87 | 28 | 29 => ("rgba", 0),
                other => return Err(tr!("format DXGI {other} non pris en charge", other).into()),
            }
        }
        _ if u32_at(84) == 32 => ("rgba", 0), // non compressé 32 bits
        other => return Err(tr!("format {other} non pris en charge", other = format!("{:?}", String::from_utf8_lossy(other))).into()),
    };
    let mut img = image::RgbaImage::new(width as u32, height as u32);
    if format == "rgba" {
        let px = r.bytes(width * height * 4)?;
        img.copy_from_slice(px);
        return Ok(img);
    }
    let (bw, bh) = (width.div_ceil(4), height.div_ceil(4));
    let mut block = [0u8; 64];
    for by in 0..bh {
        for bx in 0..bw {
            let src = r.bytes(block_bytes)?;
            match format {
                "bc1" => bcdec_rs::bc1(src, &mut block, 16),
                "bc3" => bcdec_rs::bc3(src, &mut block, 16),
                _ => bcdec_rs::bc7(src, &mut block, 16),
            }
            for y in 0..4 {
                for x in 0..4 {
                    let (px, py) = (bx * 4 + x, by * 4 + y);
                    if px < width && py < height {
                        let o = (y * 4 + x) * 4;
                        img.put_pixel(px as u32, py as u32, image::Rgba([block[o], block[o + 1], block[o + 2], block[o + 3]]));
                    }
                }
            }
        }
    }
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Petit Content.ggpk (version 3, noms UTF-16) : racine → Bundles2 → _.index.bin et Folders/a.bundle.bin.
    #[test]
    fn ggpk_inventory() {
        fn name(n: &str) -> Vec<u8> {
            n.encode_utf16().chain([0]).flat_map(|u| u.to_le_bytes()).collect()
        }
        fn file(n: &str, data: &[u8]) -> Vec<u8> {
            let nm = name(n);
            let len = 12 + 32 + nm.len() + data.len();
            [&(len as u32).to_le_bytes()[..], b"FILE", &((nm.len() / 2) as u32).to_le_bytes(), &[0u8; 32], &nm, data].concat()
        }
        fn dir(n: &str, children: &[u64]) -> Vec<u8> {
            let nm = name(n);
            let entries: Vec<u8> = children.iter().flat_map(|c| [&0u32.to_le_bytes()[..], &c.to_le_bytes()].concat()).collect();
            let len = 12 + 4 + 32 + nm.len() + entries.len();
            [&(len as u32).to_le_bytes()[..], b"PDIR", &((nm.len() / 2) as u32).to_le_bytes(), &(children.len() as u32).to_le_bytes(), &[0u8; 32], &nm, &entries].concat()
        }
        let mut out = vec![0u8; 28];
        let index_at = out.len() as u64;
        out.extend(file("_.index.bin", b"INDEX"));
        let bundle_at = out.len() as u64;
        out.extend(file("a.bundle.bin", b"BUNDLE"));
        let folders_at = out.len() as u64;
        out.extend(dir("Folders", &[bundle_at]));
        let b2_at = out.len() as u64;
        out.extend(dir("Bundles2", &[index_at, folders_at]));
        let root_at = out.len() as u64;
        out.extend(dir("", &[b2_at]));
        out[0..28].copy_from_slice(&[&28u32.to_le_bytes()[..], b"GGPK", &3u32.to_le_bytes(), &0u64.to_le_bytes(), &root_at.to_le_bytes()].concat());
        let path = std::env::temp_dir().join(format!("poe2-tracker-test-{}.ggpk", std::process::id()));
        std::fs::write(&path, &out).unwrap();
        let files = ggpk_bundles(&path);
        let source = Source::Ggpk(path.clone(), files.unwrap());
        assert_eq!(source.read("_.index.bin").unwrap(), b"INDEX");
        assert_eq!(source.read("Folders/a.bundle.bin").unwrap(), b"BUNDLE");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn paths_with_bases() {
        // base "Art/" puis chemin "1:" + "a.dds" => "Art/a.dds"
        let mut data = Vec::new();
        data.extend(0u32.to_le_bytes());
        data.extend(1u32.to_le_bytes());
        data.extend(b"Art/\0");
        data.extend(0u32.to_le_bytes());
        data.extend(1u32.to_le_bytes()); // 1 = première base
        data.extend(b"a.dds\0");
        let mut out = Vec::new();
        read_paths(&data, &mut out).unwrap();
        assert_eq!(out, vec!["Art/a.dds"]);
    }

    /// Liste les fichiers du jeu dont le chemin contient un motif (exploration des données) :
    /// POE2_GAME=…/Path\ of\ Exile\ 2 POE2_GREP=atlas cargo test list_game_paths -- --ignored --nocapture
    #[test]
    #[ignore]
    fn list_game_paths() {
        let (Ok(game), Ok(grep)) = (std::env::var("POE2_GAME"), std::env::var("POE2_GREP")) else { return };
        let index = Index::open(Path::new(&game)).unwrap();
        let grep = grep.to_lowercase();
        for p in index.paths.iter().filter(|p| p.to_lowercase().contains(&grep)) {
            eprintln!("{p}  ({} o)", index.entry(p).map_or(0, |e| e.size));
        }
    }

    /// Copie un fichier du jeu sur le disque (exploration des données) :
    /// POE2_GAME=… POE2_FILE=metadata/… POE2_OUT=/tmp/x cargo test dump_game_file -- --ignored
    #[test]
    #[ignore]
    fn dump_game_file() {
        let (Ok(game), Ok(file), Ok(out)) = (std::env::var("POE2_GAME"), std::env::var("POE2_FILE"), std::env::var("POE2_OUT")) else { return };
        let index = Index::open(Path::new(&game)).unwrap();
        for f in file.split(',') {
            let name = f.rsplit('/').next().unwrap();
            std::fs::write(Path::new(&out).join(name), index.read(f).unwrap()).unwrap();
        }
    }
}
