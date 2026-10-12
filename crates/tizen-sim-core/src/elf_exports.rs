//! Hiding symbols a Linux Engine exports from the system libraries it loads.
//!
//! Some old Chromium builds export the libraries they link statically, e.g.
//! M56's HarfBuzz (`hb_*`). The dynamic loader binds the system's Pango to
//! those, not to the system HarfBuzz it was built against, and the Engine
//! crashes while GTK lays out its first text (measured on Debian 13 and
//! Ubuntu 22.04). Renaming them in the executable's dynamic string table
//! leaves the Engine's own calls alone (they don't go through the table) and
//! makes the loader find the system's.

/// Renames the dynamic symbols whose names start with one of `prefixes`, by
/// replacing their first character with `_`, and returns how many it renamed.
pub(crate) fn unexport(elf: &mut [u8], prefixes: &[String]) -> Result<usize, String> {
    let (start, end) = dynstr_range(elf)?;
    let table = &mut elf[start..end];
    let mut renamed = 0;
    // Names start at offset 0 or right after a NUL.
    for at in 0..table.len() {
        let starts_name = at == 0 || table[at - 1] == 0;
        if starts_name
            && prefixes
                .iter()
                .any(|prefix| table[at..].starts_with(prefix.as_bytes()))
        {
            table[at] = b'_';
            renamed += 1;
        }
    }
    Ok(renamed)
}

/// Where the `.dynstr` section lies in a 64-bit little-endian ELF file.
fn dynstr_range(elf: &[u8]) -> Result<(usize, usize), String> {
    if elf.len() < 64 || &elf[..4] != b"\x7fELF" || elf[4] != 2 || elf[5] != 1 {
        return Err("not a 64-bit little-endian ELF file".into());
    }
    let u16_at = |at: usize| u16::from_le_bytes([elf[at], elf[at + 1]]) as usize;
    let u32_at = |at: usize| -> Option<usize> {
        Some(u32::from_le_bytes(elf.get(at..at + 4)?.try_into().ok()?) as usize)
    };
    let u64_at = |at: usize| -> Option<usize> {
        usize::try_from(u64::from_le_bytes(elf.get(at..at + 8)?.try_into().ok()?)).ok()
    };
    let truncated = || "truncated ELF section headers".to_string();
    let section_headers = u64_at(0x28).ok_or_else(truncated)?;
    let (entry_size, count, names_index) = (u16_at(0x3a), u16_at(0x3c), u16_at(0x3e));
    // (offset, size) of section `index`.
    let section = |index: usize| -> Option<(usize, usize)> {
        let header = section_headers.checked_add(index.checked_mul(entry_size)?)?;
        let (offset, size) = (u64_at(header + 0x18)?, u64_at(header + 0x20)?);
        (offset.checked_add(size)? <= elf.len()).then_some((offset, size))
    };
    let (names, names_size) = section(names_index).ok_or_else(truncated)?;
    for index in 0..count {
        let header = section_headers + index * entry_size;
        let name = u32_at(header).ok_or_else(truncated)?;
        if name < names_size && elf[names + name..names + names_size].starts_with(b".dynstr\0") {
            let (offset, size) = section(index).ok_or_else(truncated)?;
            return Ok((offset, offset + size));
        }
    }
    Err("the ELF file has no .dynstr section".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 64-bit little-endian ELF with only section headers: the null
    /// section, `.shstrtab` and `.dynstr` holding `names`.
    fn elf_with_dynstr(names: &[&str]) -> Vec<u8> {
        let shstrtab = b"\0.shstrtab\0.dynstr\0".to_vec();
        let mut dynstr = vec![0u8];
        for name in names {
            dynstr.extend_from_slice(name.as_bytes());
            dynstr.push(0);
        }
        let header_size = 64;
        let shstrtab_offset = header_size;
        let dynstr_offset = shstrtab_offset + shstrtab.len();
        let section_headers = dynstr_offset + dynstr.len();

        let mut elf = vec![0u8; header_size];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2; // ELFCLASS64
        elf[5] = 1; // ELFDATA2LSB
        elf[0x28..0x30].copy_from_slice(&(section_headers as u64).to_le_bytes());
        elf[0x3a..0x3c].copy_from_slice(&64u16.to_le_bytes());
        elf[0x3c..0x3e].copy_from_slice(&3u16.to_le_bytes());
        elf[0x3e..0x40].copy_from_slice(&1u16.to_le_bytes());
        elf.extend_from_slice(&shstrtab);
        elf.extend_from_slice(&dynstr);
        let section = |name: u32, kind: u32, offset: usize, size: usize| {
            let mut header = vec![0u8; 64];
            header[..4].copy_from_slice(&name.to_le_bytes());
            header[4..8].copy_from_slice(&kind.to_le_bytes());
            header[0x18..0x20].copy_from_slice(&(offset as u64).to_le_bytes());
            header[0x20..0x28].copy_from_slice(&(size as u64).to_le_bytes());
            header
        };
        elf.extend(section(0, 0, 0, 0));
        elf.extend(section(1, 3, shstrtab_offset, shstrtab.len()));
        elf.extend(section(11, 3, dynstr_offset, dynstr.len()));
        elf
    }

    fn dynstr_names(elf: &[u8]) -> Vec<String> {
        let start = 64 + b"\0.shstrtab\0.dynstr\0".len() + 1;
        let end = u64::from_le_bytes(elf[0x28..0x30].try_into().unwrap()) as usize;
        elf[start..end - 1]
            .split(|&b| b == 0)
            .map(|name| String::from_utf8(name.to_vec()).unwrap())
            .collect()
    }

    #[test]
    fn renames_only_names_that_start_with_a_prefix() {
        let mut elf = elf_with_dynstr(&["hb_buffer_create", "ChromeMain", "not_hb_x", "hb_font"]);

        let renamed = unexport(&mut elf, &["hb_".to_string()]).unwrap();

        assert_eq!(renamed, 2);
        assert_eq!(
            dynstr_names(&elf),
            ["_b_buffer_create", "ChromeMain", "not_hb_x", "_b_font"]
        );
    }

    #[test]
    fn leaves_the_rest_of_the_file_alone() {
        let original = elf_with_dynstr(&["hb_a", "b"]);
        let mut elf = original.clone();

        unexport(&mut elf, &["hb_".to_string()]).unwrap();

        let differing: Vec<usize> = (0..elf.len()).filter(|&i| elf[i] != original[i]).collect();
        assert_eq!(differing.len(), 1);
    }

    #[test]
    fn a_file_that_is_not_a_64_bit_elf_is_an_error() {
        let error = unexport(&mut b"MZ\x90\x00".to_vec(), &["hb_".to_string()]).unwrap_err();
        assert!(error.contains("ELF"), "{error}");
    }

    #[test]
    fn an_elf_without_a_dynamic_string_table_is_an_error() {
        let mut elf = elf_with_dynstr(&["hb_a"]);
        // Rename `.dynstr` to `.dynstx`.
        let at = 64 + b"\0.shstrtab\0.dynst".len();
        elf[at] = b'x';

        let error = unexport(&mut elf, &["hb_".to_string()]).unwrap_err();
        assert!(error.contains(".dynstr"), "{error}");
    }
}
