use resvg::{tiny_skia, usvg};
use std::{env, fs, path::PathBuf};

fn main() {
    const MARK: &str = "assets/brand/chronver-mark.svg";
    println!("cargo:rerun-if-changed={MARK}");
    let target = env::var("TARGET").unwrap();
    if !target.ends_with("-windows-msvc") {
        return;
    }

    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32">{}</svg>"#,
        include_str!("assets/brand/chronver-mark.svg"),
    );
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).expect("valid Chronver mark");
    let sizes = [16u32, 20, 24, 32, 48, 64, 256];
    let frames = sizes.map(|size| {
        let mut pixmap = tiny_skia::Pixmap::new(size, size).unwrap();
        let scale = size as f32 / 32.;
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        pixmap.encode_png().expect("encode Chronver icon frame")
    });

    // ICO directory entries point to PNG frames, which Windows Vista and later support.
    let mut ico = vec![0, 0, 1, 0];
    ico.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (size, frame) in sizes.into_iter().zip(&frames) {
        ico.extend_from_slice(&[size as u8, size as u8, 0, 0, 1, 0, 32, 0]);
        ico.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += frame.len() as u32;
    }
    for frame in frames {
        ico.extend(frame);
    }

    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("chronver.ico"), ico).expect("write the Windows icon");
    fs::write(out.join("chronver.rc"), "1 ICON \"chronver.ico\"\n")
        .expect("write the icon resource script");
    let resource = out.join("chronver.res");
    let status = find_msvc_tools::find_tool(&target, "rc.exe")
        .expect("the Windows SDK resource compiler is required for the Chronver icon")
        .to_command()
        .current_dir(out)
        .arg("/nologo")
        .arg("/fo")
        .arg(&resource)
        .arg("chronver.rc")
        .status()
        .expect("run the Windows SDK resource compiler");
    assert!(
        status.success(),
        "Windows SDK could not compile the Chronver icon"
    );
    println!(
        "cargo:rustc-link-arg-bin=chronver-desktop={}",
        resource.display()
    );
}
