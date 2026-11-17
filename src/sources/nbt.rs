//! A structure template's palette: the block names in its gzipped NBT (`palette`, or each list of `palettes`).
//! Only what the World Gen page needs is read; a file that doesn't parse gives no blocks (it must never panic, it
//! runs in a server reload listener).

/// The most a template may inflate to (vanilla's largest are a few hundred KiB).
const MAX: usize = 32 << 20;

/// The block names of a gzipped structure template.
pub(crate) fn palette(gz: &[u8]) -> Vec<String> {
    let Some(raw) = gunzip(gz) else { return Vec::new() };
    let mut r = Reader { b: &raw, at: 0 };
    let mut out = Vec::new();
    // The root: a named compound.
    if r.u8() != Some(10) || r.string().is_none() {
        return out;
    }
    let _ = r.compound(0, &mut |path, name| {
        if path == Some("palette") || path == Some("palettes") {
            out.push(name);
        }
    });
    out.sort();
    out.dedup();
    out
}

/// Skips a gzip member's header and inflates its deflate body.
fn gunzip(b: &[u8]) -> Option<Vec<u8>> {
    if b.len() < 18 || b[0] != 0x1f || b[1] != 0x8b || b[2] != 8 {
        return None;
    }
    let flags = b[3];
    let mut at = 10usize;
    if flags & 4 != 0 {
        let n = u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]) as usize;
        at += 2 + n;
    }
    for bit in [8u8, 16] {
        if flags & bit != 0 {
            at += b.get(at..)?.iter().position(|c| *c == 0)? + 1;
        }
    }
    if flags & 2 != 0 {
        at += 2;
    }
    miniz_oxide::inflate::decompress_to_vec_with_limit(b.get(at..)?, MAX).ok()
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|s| s[0])
    }
    fn i32(&mut self) -> Option<i32> {
        self.take(4).map(|s| i32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn string(&mut self) -> Option<String> {
        let n = self.take(2).map(|s| u16::from_be_bytes([s[0], s[1]]) as usize)?;
        self.take(n).map(|s| String::from_utf8_lossy(s).into_owned())
    }
    /// Skips or reads one payload of `kind`. `list` names the list a compound is in, so a palette entry's `Name`
    /// can be told from any other `Name`.
    fn payload(&mut self, kind: u8, depth: u32, list: Option<&str>, found: &mut dyn FnMut(Option<&str>, String)) -> Option<()> {
        if depth > 64 {
            return None;
        }
        match kind {
            1 => self.take(1).map(drop),
            2 => self.take(2).map(drop),
            3 | 5 => self.take(4).map(drop),
            4 | 6 => self.take(8).map(drop),
            7 => {
                let n = usize::try_from(self.i32()?).ok()?;
                self.take(n).map(drop)
            }
            8 => self.string().map(drop),
            9 => {
                let inner = self.u8()?;
                let n = usize::try_from(self.i32()?).ok()?;
                for _ in 0..n {
                    self.payload(inner, depth + 1, list, found)?;
                }
                Some(())
            }
            10 => self.compound_in(depth + 1, list, found),
            11 => {
                let n = usize::try_from(self.i32()?).ok()?;
                self.take(n.checked_mul(4)?).map(drop)
            }
            12 => {
                let n = usize::try_from(self.i32()?).ok()?;
                self.take(n.checked_mul(8)?).map(drop)
            }
            _ => None,
        }
    }
    fn compound(&mut self, depth: u32, found: &mut dyn FnMut(Option<&str>, String)) -> Option<()> {
        self.compound_in(depth, None, found)
    }
    /// A compound's entries; a string `Name` in a compound of the `palette` or `palettes` list is a block.
    fn compound_in(&mut self, depth: u32, list: Option<&str>, found: &mut dyn FnMut(Option<&str>, String)) -> Option<()> {
        loop {
            let kind = self.u8()?;
            if kind == 0 {
                return Some(());
            }
            let name = self.string()?;
            if kind == 8 && name == "Name" && list.is_some() {
                let value = self.string()?;
                found(list, value);
                continue;
            }
            // Lists keep their own name for the compounds in them; a list of lists (`palettes`) passes it on.
            let inner_list = if kind == 9 && (name == "palette" || name == "palettes") {
                Some(if name == "palette" { "palette" } else { "palettes" })
            } else if kind == 9 {
                None
            } else {
                list.filter(|_| kind == 9)
            };
            self.payload(kind, depth, inner_list, found)?;
        }
    }
}
