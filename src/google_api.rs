use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::process::Command;

use crate::OS;

#[derive(Serialize, Deserialize, Debug)]
pub struct MilestoneVersions {
    pub timestamp: String,
    pub milestones: HashMap<String, Milestone>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Milestone {
    pub milestone: String,
    pub version: String,
    pub revision: String,
}

pub async fn get_chrome_version(os: OS) -> anyhow::Result<String> {
    log::info!("Getting installed Chrome version...");
    let command = match os {
        OS::Linux => {
            Command::new("google-chrome-stable")
                .arg("--version")
                .output()
                .await?
        }
        OS::MacOS => {
            Command::new("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
                .arg("--version")
                .output()
                .await?
        }
        OS::Windows => Command::new("powershell")
            .arg("-c")
            .arg("(Get-Item 'C:/Program Files/Google/Chrome/Application/chrome.exe').VersionInfo")
            .output()
            .await?,
    };
    let output = String::from_utf8(command.stdout)?;

    let version = output
        .lines()
        .flat_map(|line| line.chars().filter(|&ch| ch.is_ascii_digit()))
        .take(3)
        .collect::<String>();

    log::info!("currently installed Chrome version: {}", version);
    Ok(version)
}

pub async fn fetch_chromedriver(client: &reqwest::Client, os: OS) -> anyhow::Result<()> {
    let installed_version = get_chrome_version(os).await?;
    let chromedriver_url: String;
    if installed_version.as_str() >= "114" {
        // Fetch the correct version
        let url = "https://googlechromelabs.github.io/chrome-for-testing/latest-versions-per-milestone.json";
        let resp = client.get(url).send().await?;
        let milestone_versions: MilestoneVersions = resp.json().await?;
        let version = milestone_versions
            .milestones
            .get(&installed_version)
            .ok_or_else(|| {
                anyhow!(
                    "Could not find version {} in the latest-versions-per-milestone.json file",
                    installed_version
                )
            })?
            .version
            .as_str();

        // Fetch the chromedriver binary
        chromedriver_url = match os {
            OS::Linux => format!(
                "https://storage.googleapis.com/chrome-for-testing-public/{}/linux64/chromedriver-linux64.zip",
                version
            ),
            OS::MacOS => format!(
                "https://storage.googleapis.com/chrome-for-testing-public/{}/mac-arm64/chromedriver-mac-arm64.zip",
                version
            ),
            OS::Windows => format!(
                "https://storage.googleapis.com/chrome-for-testing-public/{}/win64/chrome-win64.zip",
                version,
            ),
        };
    } else {
        let resp = client
            .get(format!(
                "https://chromedriver.storage.googleapis.com/LATEST_RELEASE_{}",
                installed_version
            ))
            .send()
            .await?;
        let body = resp.text().await?;
        chromedriver_url = match os {
            OS::Linux => format!(
                "https://chromedriver.storage.googleapis.com/{}/chromedriver_linux64.zip",
                body
            ),
            OS::Windows => format!(
                "https://chromedriver.storage.googleapis.com/{}/chromedriver_win32.zip",
                body
            ),
            OS::MacOS => format!(
                "https://chromedriver.storage.googleapis.com/{}/chromedriver_mac64.zip",
                body
            ),
        };
    }

    let resp = client.get(&chromedriver_url).send().await?;
    resp.error_for_status_ref()?;
    let body = resp.bytes().await?;
    unzip_chromedriver(body.to_vec())?;
    Ok(())
}

fn unzip_chromedriver(body: Vec<u8>) -> anyhow::Result<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(body))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = file.mangled_name();
        if file.name().ends_with('/') {
            std::fs::create_dir_all(&outpath)?;
        } else {
            let outpath_relative = outpath.file_name().ok_or_else(|| {
                anyhow!(
                    "couldn't get file name from path: {}",
                    outpath.to_string_lossy()
                )
            })?;
            let mut outfile = std::fs::File::create(outpath_relative)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }
    Ok(())
}
