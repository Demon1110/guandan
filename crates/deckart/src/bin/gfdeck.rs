//! `gfdeck` 命令行：render / render-all / pack / unpack / info / verify / png。
//!
//! 用法示例：
//! ```text
//! gfdeck render --theme themes/defaults --face H8 --level 8 -o out/H8.svg
//! gfdeck render-all --theme themes/defaults --level 8 -o out
//! gfdeck pack themes/defaults -o themes/defaults.gfdeck --sign-key <hex32>
//! gfdeck unpack themes/defaults.gfdeck -o /tmp/defaults
//! gfdeck info themes/defaults.gfdeck
//! ```

use std::path::{Path, PathBuf};

use ed25519_dalek::{SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

use gf_deckart::error::DeckError;
use gf_deckart::pack::{builtin_key, pack_dir, read_info, unpack, PackOptions};
use gf_deckart::{Face, LevelState, Rank, Theme};

fn main() {
    if let Err(err) = run() {
        eprintln!("[{}] {}", err.code(), err);
        std::process::exit(1);
    }
}

fn run() -> Result<(), DeckError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().cloned() else {
        print_usage();
        return Ok(());
    };
    let rest = &args[1..];
    match cmd.as_str() {
        "render" => cmd_render(rest),
        "render-all" => cmd_render_all(rest),
        "pack" => cmd_pack(rest),
        "unpack" => cmd_unpack(rest),
        "info" => cmd_info(rest),
        "verify" => cmd_verify(rest),
        "png" => cmd_png(rest),
        "-h" | "--help" | "help" => {
            print_usage();
            Ok(())
        }
        other => Err(DeckError::FaceInvalid(format!("未知子命令: {other}"))),
    }
}

fn print_usage() {
    println!(
        "gfdeck {}\n\
         子命令：\n\
         \x20 render      --theme <dir> --face <H8|JOKER-B> [--level <A|2..K>] [--corner 260] -o <file>\n\
         \x20 render-all  --theme <dir> [--level <rank>] [--corner <pct>] -o <dir>\n\
         \x20 pack        <theme-dir> -o <file.gfdeck> [--key-file <f>] [--key-id <id>] [--sign-key <hex32>]\n\
         \x20 unpack      <file.gfdeck> -o <dir> [--key-file <f>] [--verify-key <hex64>]\n\
         \x20 info        <file.gfdeck>\n\
         \x20 verify      <file.gfdeck> [--key-file <f>] [--verify-key <hex64>]\n\
         \x20 png         <file.svg> -o <file.png> [--scale 2]   （需 --features png）",
        gf_deckart::VERSION
    );
}

fn opt(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn require(args: &[String], key: &str) -> Result<String, DeckError> {
    opt(args, key).ok_or_else(|| DeckError::FaceInvalid(format!("缺少参数 {key}")))
}

fn parse_level(args: &[String]) -> Result<Option<LevelState>, DeckError> {
    match opt(args, "--level") {
        None => Ok(None),
        Some(v) if v.eq_ignore_ascii_case("none") => Ok(None),
        Some(v) => {
            let rank = Rank::from_label(&v)
                .ok_or_else(|| DeckError::FaceInvalid(format!("--level 无效: {v}")))?;
            Ok(Some(LevelState::new(rank)))
        }
    }
}

/// 角标字号，按百分比给（100 = 主题原样，260 = 2.6 倍）。缺省 100。
fn parse_corner(args: &[String]) -> Result<f64, DeckError> {
    match opt(args, "--corner") {
        None => Ok(1.0),
        Some(v) => {
            let pct: f64 = v
                .trim_end_matches('%')
                .parse()
                .map_err(|_| DeckError::FaceInvalid(format!("--corner 无效: {v}")))?;
            Ok(pct / 100.0)
        }
    }
}

fn cmd_render(args: &[String]) -> Result<(), DeckError> {
    let theme = Theme::load_dir(Path::new(&require(args, "--theme")?))?;
    let face = Face::parse(&require(args, "--face")?)?;
    let level = parse_level(args)?;
    let corner = parse_corner(args)?;
    let out = PathBuf::from(require(args, "-o")?);
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let svg = gf_deckart::render_face_opts(
        &theme,
        face,
        gf_deckart::FaceOptions {
            level,
            corner_scale: corner,
        },
    )?;
    std::fs::write(&out, &svg)?;
    println!(
        "{} → {}（{} 字节，级别 {}，角标 {:.0}%）",
        face.code(),
        out.display(),
        svg.len(),
        level.map(|l| l.level_rank.label()).unwrap_or("none"),
        corner * 100.0
    );
    Ok(())
}

fn cmd_render_all(args: &[String]) -> Result<(), DeckError> {
    let theme = Theme::load_dir(Path::new(&require(args, "--theme")?))?;
    let level = parse_level(args)?;
    let corner = parse_corner(args)?;
    let out_dir = PathBuf::from(require(args, "-o")?);
    std::fs::create_dir_all(&out_dir)?;

    let mut manifest_faces = Vec::new();
    let opts = gf_deckart::FaceOptions {
        level,
        corner_scale: corner,
    };
    for rendered in gf_deckart::render_all_opts(&theme, opts)? {
        let file = format!("{}.svg", rendered.code);
        std::fs::write(out_dir.join(&file), &rendered.svg)?;
        manifest_faces.push(format!(
            "    {{\"code\": \"{}\", \"file\": \"{}\", \"sha256\": \"{}\"}}",
            rendered.code,
            file,
            hex(&Sha256::digest(rendered.svg.as_bytes()))
        ));
    }
    let back_svg = gf_deckart::render_back(&theme)?;
    std::fs::write(out_dir.join("BACK.svg"), &back_svg)?;

    let manifest = format!(
        "{{\n  \"themeId\": \"{}\",\n  \"themeVersion\": {},\n  \"level\": \"{}\",\n  \"cornerScale\": {},\n  \"layout\": \"{:?}\",\n  \"faces\": [\n{}\n  ],\n  \"back\": {{\"file\": \"BACK.svg\", \"sha256\": \"{}\"}}\n}}\n",
        theme.meta().id,
        theme.meta().version,
        level.map(|l| l.level_rank.label()).unwrap_or("none"),
        num_json(corner),
        theme.meta().layout,
        manifest_faces.join(",\n"),
        hex(&Sha256::digest(back_svg.as_bytes()))
    );
    std::fs::write(out_dir.join("manifest.json"), manifest)?;
    println!(
        "55 张（54 面 + 牌背）已写入 {}，角标 {:.0}%，manifest.json 已生成",
        out_dir.display(),
        corner * 100.0
    );
    Ok(())
}

/// 极简数值序列化：整数不带小数点，其余保留 2 位，避免 manifest 里出现 `1.0000000000000002`。
fn num_json(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.2}")
    }
}

fn load_key(args: &[String]) -> Result<([u8; 32], String), DeckError> {
    match opt(args, "--key-file") {
        Some(path) => {
            let raw = std::fs::read(&path)?;
            let key = decode_32(&raw)?;
            let key_id = opt(args, "--key-id").unwrap_or_else(|| "file".to_string());
            Ok((key, key_id))
        }
        None => Ok(builtin_key()),
    }
}

fn decode_32(raw: &[u8]) -> Result<[u8; 32], DeckError> {
    if raw.len() == 32 {
        let mut k = [0u8; 32];
        k.copy_from_slice(raw);
        return Ok(k);
    }
    let text = String::from_utf8_lossy(raw);
    let trimmed = text.trim();
    if trimmed.len() == 64 {
        let mut k = [0u8; 32];
        for i in 0..32 {
            k[i] = u8::from_str_radix(&trimmed[i * 2..i * 2 + 2], 16)
                .map_err(|_| DeckError::PackCorrupt("密钥 hex 非法".into()))?;
        }
        return Ok(k);
    }
    Err(DeckError::PackCorrupt(format!(
        "密钥长度非法：需 32 字节二进制或 64 位 hex，实际 {}",
        raw.len()
    )))
}

fn load_signing_key(args: &[String]) -> Result<Option<SigningKey>, DeckError> {
    match opt(args, "--sign-key") {
        None => Ok(None),
        Some(v) => {
            let bytes = decode_32(v.as_bytes())?;
            Ok(Some(SigningKey::from_bytes(&bytes)))
        }
    }
}

fn load_verify_key(args: &[String]) -> Result<Option<VerifyingKey>, DeckError> {
    match opt(args, "--verify-key") {
        None => Ok(None),
        Some(v) => {
            let bytes = decode_hex_any(&v)?;
            let arr: [u8; 32] = bytes.try_into().map_err(|_| DeckError::PackSignature)?;
            VerifyingKey::from_bytes(&arr)
                .map(Some)
                .map_err(|_| DeckError::PackSignature)
        }
    }
}

fn decode_hex_any(s: &str) -> Result<Vec<u8>, DeckError> {
    let t = s.trim();
    if t.len() % 2 != 0 {
        return Err(DeckError::PackSignature);
    }
    let mut out = Vec::with_capacity(t.len() / 2);
    for i in 0..t.len() / 2 {
        out.push(
            u8::from_str_radix(&t[i * 2..i * 2 + 2], 16).map_err(|_| DeckError::PackSignature)?,
        );
    }
    Ok(out)
}

fn cmd_pack(args: &[String]) -> Result<(), DeckError> {
    let dir = PathBuf::from(
        args.iter()
            .find(|a| !a.starts_with('-'))
            .ok_or_else(|| DeckError::FaceInvalid("缺少主题目录".into()))?,
    );
    let out = PathBuf::from(require(args, "-o")?);
    let (key, key_id) = load_key(args)?;
    let key_id = opt(args, "--key-id").unwrap_or(key_id);
    let signing = load_signing_key(args)?;
    let opts = PackOptions {
        key: &key,
        key_id: &key_id,
        signing_key: signing.as_ref(),
    };
    let info = pack_dir(&dir, &out, &opts)?;
    println!(
        "已打包 {} → {}（{} 条目，{} 字节，加密={}，签名={}，keyId={}）",
        info.theme_id,
        out.display(),
        info.entries,
        info.bytes,
        info.encrypted,
        info.signed,
        info.key_id
    );
    if let Some(sk) = signing {
        let vk_hex = hex(sk.verifying_key().as_bytes());
        if let Some(path) = opt(args, "--pub-key-out") {
            std::fs::write(&path, &vk_hex)?;
            println!("公钥已写入 {path}：{vk_hex}");
        } else {
            println!("公钥（分发 verify 用）：{vk_hex}");
        }
    }
    Ok(())
}

fn cmd_unpack(args: &[String]) -> Result<(), DeckError> {
    let pack = PathBuf::from(
        args.iter()
            .find(|a| !a.starts_with('-'))
            .ok_or_else(|| DeckError::FaceInvalid("缺少 .gfdeck 路径".into()))?,
    );
    let out = PathBuf::from(require(args, "-o")?);
    let (key, _) = load_key(args)?;
    let vk = load_verify_key(args)?;
    let info = unpack(&pack, &out, &key, vk.as_ref())?;
    println!(
        "已解包 {}@{} → {}（{} 条目）",
        info.theme_id,
        info.theme_version,
        out.display(),
        info.entries
    );
    Ok(())
}

fn cmd_info(args: &[String]) -> Result<(), DeckError> {
    let pack = PathBuf::from(
        args.iter()
            .find(|a| !a.starts_with('-'))
            .ok_or_else(|| DeckError::FaceInvalid("缺少 .gfdeck 路径".into()))?,
    );
    let info = read_info(&pack)?;
    println!(
        "{}@{}（license={}，{} 条目，{} 字节，加密={}，签名={}，keyId={}）",
        info.theme_id,
        info.theme_version,
        info.license,
        info.entries,
        info.bytes,
        info.encrypted,
        info.signed,
        info.key_id
    );
    Ok(())
}

fn cmd_verify(args: &[String]) -> Result<(), DeckError> {
    let pack = PathBuf::from(
        args.iter()
            .find(|a| !a.starts_with('-'))
            .ok_or_else(|| DeckError::FaceInvalid("缺少 .gfdeck 路径".into()))?,
    );
    let info = read_info(&pack)?;
    let (key, _) = load_key(args)?;
    let vk = load_verify_key(args)?;
    // open() 会逐条解密并比对 sha256；签名在 flags 含签名时强制校验
    let theme = gf_deckart::pack::open(&pack, &key, vk.as_ref())?;
    println!(
        "校验通过：{}@{}，{} 条目，{} 面可渲染，加密={}，签名={}",
        info.theme_id,
        info.theme_version,
        info.entries,
        gf_deckart::render_all(&theme, None)?.len(),
        info.encrypted,
        info.signed
    );
    Ok(())
}

fn cmd_png(args: &[String]) -> Result<(), DeckError> {
    #[cfg(not(feature = "png"))]
    {
        let _ = args;
        Err(DeckError::Png(
            "本二进制未启用 PNG 支持：用 cargo build --features png 重新构建".into(),
        ))
    }
    #[cfg(feature = "png")]
    {
        let svg_path = args
            .iter()
            .find(|a| !a.starts_with('-'))
            .ok_or_else(|| DeckError::Png("缺少 svg 路径".into()))?;
        let svg = std::fs::read_to_string(svg_path)?;
        let scale: f64 = opt(args, "--scale")
            .map(|v| v.parse::<f64>())
            .transpose()
            .map_err(|e| DeckError::Png(format!("--scale 非法: {e}")))?
            .unwrap_or(1.0);
        let out = PathBuf::from(require(args, "-o")?);
        let png = gf_deckart::png::svg_to_png(&svg, scale)?;
        if let Some(parent) = out.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(&out, &png)?;
        println!(
            "{} → {}（{} 字节，×{}）",
            svg_path,
            out.display(),
            png.len(),
            scale
        );
        Ok(())
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
