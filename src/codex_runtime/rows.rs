use super::*;

const OFFSET: usize = 0x09f9_8ea4;
const ORIGINAL_SHA: &str = "43cf8c776cb5d233478a9cb4ace74d7f002753630a2d8b6db90c4159fc5d43de";
const SIGNED_SHA: &str = "33937eecc47a26ca0d1a896ef8b4ae3bb9ae04a7d87a27db0dc2f95dc038da4a";
const SIGNED_SIZE: u64 = 356_842_784;
const ORIGINAL: [u32; 16] = [
    0xf100091f, 0x540002e8, 0xb40001d5, 0xa9400680, 0x97e9d197, 0xb4000261, 0xf10006bf, 0x54000120,
    0xa9410680, 0x97e9d192, 0xb40001c1, 0xf1000abf, 0x54000080, 0xa9420680, 0x97e9d18d, 0xb4000121,
];
// Codex 0.165.0 cxstatusline_parse_output at VA 0x109f98e38. The compiler
// unrolled the nonblank check for three rows, so changing only the bound is
// insufficient. x22 is already saved by the function's prologue. This loop
// preserves the original trim call and validates every row before conversion.
const REPLACEMENT: [u32; 16] = [
    0xf100151f, // cmp x8, #5 (unsigned rows.len() - 1 <= 5)
    0x540002e8, // b.hi failure
    0xaa1403f6, // mov x22, x20
    0xa8c106c0, // loop: ldp x0, x1, [x22], #16
    0x97e9d197, // bl str::trim_matches(is_whitespace)
    0xb4000261, // cbz x1, failure
    0x8b151288, // add x8, x20, x21, lsl #4
    0xeb0802df, // cmp x22, x8
    0x54ffff61, // b.ne loop
    0x14000007, // b conversion
    0xd503201f, 0xd503201f, 0xd503201f, 0xd503201f, 0xd503201f, 0xd503201f,
];

// pre_draw_tick must poll external state even when Codex's own payload is clean.
// Keep its pending guard and the separate 60-second API limit gate untouched.
// Change only this local poll's 200ms duration to 500ms (including the
// nanosecond carry constant); event-driven renders retain their 200ms throttle.
const REFRESH_PATCHES: [(usize, u32, u32); 5] = [
    (0x09f0_ce9c, 0x36000808, 0xd503201f), // dirty=false branch -> nop
    (0x09f0_cecc, 0x52984015, 0x528ca015), // mov w21, low(500_000_000)
    (0x09f0_ced0, 0x72a17d75, 0x72a3b9b5), // movk w21, high(500_000_000)
    (0x09f0_cf40, 0x529f000a, 0x5293600a), // mov w10, low(-500_000_000)
    (0x09f0_cf44, 0x72ba0a0a, 0x72bc464a), // movk w10, high(-500_000_000)
];

fn patch(data: &mut [u8]) -> Outcome {
    if digest(data) != ORIGINAL_SHA {
        return Err("Six-row Codex transport source checksum mismatch".into());
    }
    let before: Vec<u8> = ORIGINAL
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    let after: Vec<u8> = REPLACEMENT
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    let block = data
        .get_mut(OFFSET..OFFSET + before.len())
        .ok_or("Six-row Codex transport patch is outside the executable")?;
    if block != before {
        return Err("Six-row Codex transport instruction mismatch".into());
    }
    block.copy_from_slice(&after);
    for (offset, before, after) in REFRESH_PATCHES {
        let instruction = data
            .get_mut(offset..offset + 4)
            .ok_or("Codex refresh patch is outside the executable")?;
        if instruction != before.to_le_bytes() {
            return Err("Codex refresh instruction mismatch".into());
        }
        instruction.copy_from_slice(&after.to_le_bytes());
    }
    Ok(())
}

pub(super) fn prepare(
    source: &Path,
    platform: &str,
    sha: &str,
    size: u64,
) -> Result<PathBuf, String> {
    if platform != "darwin-arm64" {
        return Ok(source.to_path_buf());
    }
    let parent = source.parent().ok_or("Invalid Codex package path")?;
    let target = parent.join(format!("{VERSION}-{platform}-six-rows-v2"));
    let manifest = fs::read(source.join(MANIFEST_NAME)).map_err(|e| e.to_string())?;
    let mut artifact = parse_manifest(&manifest, platform, sha, size)?;
    let binary = artifact
        .files
        .get_mut("bin/codex")
        .ok_or("Codex executable missing")?;
    if binary.sha256 != ORIGINAL_SHA {
        return Err("Unsupported six-row Codex transport source".into());
    }
    binary.sha256 = SIGNED_SHA.into();
    binary.size = SIGNED_SIZE;
    if target.exists() {
        verify_tree(&target, &artifact)?;
        return Ok(target);
    }
    let stage = (0..1000)
        .find_map(|n| {
            let path = parent.join(format!(".rows-stage-{}-{n}", std::process::id()));
            #[cfg(unix)]
            let mut builder = fs::DirBuilder::new();
            #[cfg(not(unix))]
            let builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(&path).ok().map(|()| path)
        })
        .ok_or("Could not create six-row Codex staging directory")?;
    let result: Outcome = (|| {
        for name in artifact.files.keys() {
            let dest = stage.join(name);
            fs::create_dir_all(dest.parent().ok_or("Invalid Codex file path")?)
                .map_err(|e| e.to_string())?;
            fs::copy(source.join(name), dest).map_err(|e| e.to_string())?;
        }
        let path = stage.join("bin/codex");
        let mut data = fs::read(&path).map_err(|e| e.to_string())?;
        patch(&mut data)?;
        fs::write(&path, data).map_err(|e| e.to_string())?;
        let result = std::process::Command::new("/usr/bin/codesign")
            .args([
                "--force",
                "--sign",
                "-",
                "--identifier",
                "clistatus-codex-six-rows",
                "--timestamp=none",
            ])
            .arg(&path)
            .output()
            .map_err(|e| e.to_string())?;
        if !result.status.success() {
            return Err(format!(
                "Could not sign six-row Codex transport: {}",
                String::from_utf8_lossy(&result.stderr)
            ));
        }
        verify_tree(&stage, &artifact)?;
        // A concurrent launcher may have completed the same verified cache.
        if target.exists() {
            verify_tree(&target, &artifact)?;
        } else if let Err(error) = fs::rename(&stage, &target) {
            if !target.exists() {
                return Err(error.to_string());
            }
            verify_tree(&target, &artifact)?;
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&stage);
    result?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unpinned_binary_without_modifying_it() {
        let mut data = vec![0; 64];
        assert!(patch(&mut data).is_err());
        assert_eq!(data, vec![0; 64]);
    }

    #[test]
    #[ignore = "requires the pinned macOS transport and codesign; writes only to a temporary directory"]
    fn derives_and_revalidates_real_transport() {
        let (platform, sha, size) = platform().unwrap();
        assert_eq!(platform, "darwin-arm64");
        let source = package_path(platform).unwrap();
        verified_package(&source, platform, sha, size).unwrap();
        let scratch = super::super::tests::scratch("six-rows");
        let local = scratch.join("original");
        fs::create_dir_all(&local).unwrap();
        let manifest = fs::read(source.join(MANIFEST_NAME)).unwrap();
        let artifact = parse_manifest(&manifest, platform, sha, size).unwrap();
        for name in artifact.files.keys() {
            let dest = local.join(name);
            fs::create_dir_all(dest.parent().unwrap()).unwrap();
            fs::copy(source.join(name), dest).unwrap();
        }
        fs::write(local.join(MANIFEST_NAME), manifest).unwrap();
        let derived = prepare(&local, platform, sha, size).unwrap();
        assert_eq!(
            hash_file(&derived.join("bin/codex")).unwrap(),
            (SIGNED_SHA.into(), SIGNED_SIZE)
        );
        assert_eq!(prepare(&local, platform, sha, size).unwrap(), derived);
        verified_package(&local, platform, sha, size).unwrap();
        fs::write(derived.join("bin/codex"), b"tampered").unwrap();
        assert!(prepare(&local, platform, sha, size).is_err());
        fs::remove_dir_all(scratch).unwrap();
    }
}
