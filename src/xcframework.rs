use crate::{Mode, Result, Target, console::Error, lib_type::LibType};
use anyhow::{Context, anyhow};
use std::{
    fs::{read_dir, remove_dir_all},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub fn search_subframework_paths(output_dir: &Path) -> Result<Vec<PathBuf>> {
    let xcf_path = read_dir(output_dir)?.flatten().find(|dir| {
        dir.file_name()
            .to_str()
            .ok_or(anyhow!(
                "The directory that is being checked if it is an XCFramework has an invalid name!"
            ))
            .is_ok_and(|dir| dir.contains(".xcframework"))
    });

    let subframework_paths = if let Some(path) = xcf_path {
        read_dir(path.path())?
            .filter_map(|subdir| {
                if subdir.as_ref().ok()?.file_type().ok()?.is_dir() {
                    Some(subdir.ok()?.path())
                } else {
                    None
                }
            })
            .collect()
    } else {
        return Err(Error::new(format!(
            "failed to find .xcframework in {}",
            output_dir.display()
        )));
    };
    Ok(subframework_paths)
}

pub fn patch_subframework(
    sf_dir: &Path,
    generated_dir: &Path,
    xcframework_name: &str,
) -> Result<()> {
    let mut headers = sf_dir.to_owned();
    headers.push("headers");
    remove_dir_all(&headers)
        .with_context(|| format!("Failed to remove unpatched directory {}", headers.display()))?;
    let mut generated_headers = generated_dir.to_owned();
    generated_headers.push("headers");

    let mut patched_headers = sf_dir.to_owned();
    patched_headers.push("headers");
    patched_headers.push(xcframework_name);
    std::fs::create_dir_all(&patched_headers).with_context(|| {
        format!(
            "Failed to create empty patched directory {}",
            patched_headers.display()
        )
    })?;

    let mut gen_header_files = Vec::<PathBuf>::new();
    for file in std::fs::read_dir(&generated_headers).with_context(|| {
        format!(
            "Failed to read from the generated header directory {}",
            patched_headers.display()
        )
    })? {
        let file = file?;
        gen_header_files.push(file.path());
    }

    for path in gen_header_files {
        let filename = path
            .components()
            .next_back()
            .ok_or(anyhow!("Expected source filename when copying"))?;
        patched_headers.push(filename);
        std::fs::copy(&path, &patched_headers).with_context(|| {
            format!(
                "Failed to copy header file from {} to {}",
                path.display(),
                patched_headers.display()
            )
        })?;
        let _copied_file = patched_headers.pop();
    }

    Ok(())
}

pub fn patch_xcframework(
    output_dir: &Path,
    generated_dir: &Path,
    xcframework_name: &str,
) -> Result<()> {
    let subframeworks =
        search_subframework_paths(output_dir).context("Failed to get subframework components")?;
    for subframework in subframeworks {
        patch_subframework(&subframework, generated_dir, xcframework_name)
            .with_context(|| format!("Failed to patch {}", subframework.display()))?;
    }

    Ok(())
}
pub fn create_xcframework(
    targets: &[Target],
    lib_name: &str,
    xcframework_name: &str,
    generated_dir: &Path,
    output_dir: &Path,
    mode: Mode,
    lib_type: LibType,
) -> Result<()> {
    /*println!(
        "Targets: {:#?}\nlib_name: {:?}\nxcframework_name: {:?}\ngenerated_dir {:?}\noutput_dir: {:?}\nmode: {:?}\nlib_type: {:?}",
        targets, lib_name, xcframework_name, generated_dir, output_dir, mode, lib_type
    );*/
    let libs: Vec<_> = targets
        .iter()
        .map(|t| t.library_path(lib_name, mode, lib_type))
        .collect();

    let headers = generated_dir.join("headers");
    let headers = headers
        .to_str()
        .ok_or(anyhow!("Directory for bindings has an invalid name!"))?;

    let output_dir_name = &output_dir
        .to_str()
        .ok_or(anyhow!("Output directory has an invalid name!"))?;

    let framework = format!("{output_dir_name}/{xcframework_name}.xcframework");

    let mut xcodebuild = Command::new("xcodebuild");
    xcodebuild.arg("-create-xcframework");

    for lib in &libs {
        xcodebuild.arg("-library");
        xcodebuild.arg(lib);
        xcodebuild.arg("-headers");
        xcodebuild.arg(headers);
    }

    let output = xcodebuild
        .arg("-output")
        .arg(&framework)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    if output.status.success() {
        patch_xcframework(output_dir, generated_dir, xcframework_name)
            .context("Failed to patch the XCFramework")?;
        Ok(())
    } else {
        Err(output.stderr.into())
    }
}
