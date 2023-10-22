mod chrome_api;

use chrome_api::MilestoneVersions;
use rand::Rng;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::os::unix::fs::PermissionsExt;
use thirtyfour::{DesiredCapabilities, WebDriver};
use tokio::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unsupported OS: `{0}`")]
    UnsupportedOS(&'static str),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    Io(#[from] tokio::io::Error),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OS {
    Linux,
    MacOS,
    Windows,
}

fn random_char() -> u8 {
    let alphabet = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ".as_bytes();
    alphabet[rand::thread_rng().gen_range(0..48)]
}

/// Fetches a new ChromeDriver executable and patches it to prevent detection.
/// Returns a WebDriver instance.
pub async fn chrome() -> Result<WebDriver, Box<dyn std::error::Error>> {
    let os = match std::env::consts::OS {
        "linux" => OS::Linux,
        "macos" => OS::MacOS,
        "windows" => OS::Windows,
        unknown_os => return Err(Error::UnsupportedOS(unknown_os).into()),
    };

    let chromedriver_exists = match os {
        OS::Linux | OS::MacOS => tokio::fs::try_exists("chromedriver").await?,
        OS::Windows => tokio::fs::try_exists("chromedriver.exe").await?,
    };

    if chromedriver_exists {
        log::info!("ChromeDriver already exists!");
    } else {
        log::info!("ChromeDriver does not exist! Fetching...");
        let client = reqwest::Client::new();
        fetch_chromedriver(&client, os).await?;
    }

    let patched_chromedriver_exec = match os {
        OS::Linux | OS::MacOS => "chromedriver_PATCHED",
        OS::Windows => "chromedriver_PATCHED.exe",
    };

    if tokio::fs::try_exists(patched_chromedriver_exec).await? {
        log::info!("patching chromedriver executable");
        let file_name = if cfg!(windows) {
            "chromedriver.exe"
        } else {
            "chromedriver"
        };
        let f = tokio::fs::read(file_name).await?;
        let mut new_chromedriver_bytes = f.clone();
        let mut total_cdc = String::from("");
        let mut cdc_pos_list = Vec::new();
        let mut is_cdc_present = false;
        let mut patch_ct = 0;
        for i in 0..f.len() - 3 {
            if "cdc_"
                == format!(
                    "{}{}{}{}",
                    f[i] as char,
                    f[i + 1] as char,
                    f[i + 2] as char,
                    f[i + 3] as char
                )
                .as_str()
            {
                for x in i + 4..i + 22 {
                    total_cdc.push_str(&(f[x] as char).to_string());
                }
                is_cdc_present = true;
                cdc_pos_list.push(i);
                total_cdc = String::from("");
            }
        }
        if is_cdc_present {
            log::info!("Found cdcs!")
        } else {
            log::info!("No cdcs were found!")
        }

        for i in cdc_pos_list {
            for x in i + 4..i + 22 {
                new_chromedriver_bytes[x] = random_char();
            }
            patch_ct += 1;
        }
        log::info!("Patched {} cdcs!", patch_ct);
        log::info!(
            "Writing patched executable to {}...",
            patched_chromedriver_exec
        );
        tokio::fs::write(patched_chromedriver_exec, new_chromedriver_bytes).await?;
        log::info!(
            "Successfully wrote patched executable to {}",
            patched_chromedriver_exec
        );
    } else {
        log::info!("Detected patched chromedriver executable!");
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let mut perms = tokio::fs::metadata(patched_chromedriver_exec)
            .await?
            .permissions();
        perms.set_mode(0o755);
        tokio::fs::set_permissions(patched_chromedriver_exec, perms).await?;
    }

    log::info!("Starting chromedriver...");
    let port: usize = rand::thread_rng().gen_range(2000..5000);
    Command::new(format!("./{}", patched_chromedriver_exec))
        .arg(format!("--port={}", port))
        .spawn()?;

    let mut caps = DesiredCapabilities::chrome();
    caps.set_no_sandbox().unwrap();
    caps.set_disable_dev_shm_usage().unwrap();
    caps.add_chrome_arg("--disable-blink-features=AutomationControlled")
        .unwrap();
    caps.add_chrome_arg("window-size=1920,1080").unwrap();
    caps.add_chrome_arg("user-agent=Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/102.0.0.0 Safari/537.36").unwrap();
    caps.add_chrome_arg("disable-infobars").unwrap();
    caps.add_chrome_option("excludeSwitches", ["enable-automation"])
        .unwrap();
    let mut attempt = 0;
    loop {
        if attempt >= 20 {
            return Err("Could not connect to chromedriver".into());
        }
        match WebDriver::new(&format!("http://localhost:{}", port), caps.clone()).await {
            Ok(d) => {
                return Ok(d);
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(250)).await,
        }
        attempt += 1;
    }
}

async fn fetch_chromedriver(
    client: &reqwest::Client,
    os: OS,
) -> Result<(), Box<dyn std::error::Error>> {
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
                format!(
                    "Could not find version {} in the latest-versions-per-milestone.json file",
                    installed_version
                )
            })?
            .version
            .as_str();

        // Fetch the chromedriver binary
        chromedriver_url = match os {
            OS::Linux => format!(
                "https://edgedl.me.gvt1.com/edgedl/chrome/chrome-for-testing/{}/{}/{}",
                version, "linux64", "chromedriver-linux64.zip"
            ),
            OS::MacOS => format!(
                "https://edgedl.me.gvt1.com/edgedl/chrome/chrome-for-testing/{}/{}/{}",
                version, "mac-x64", "chromedriver-mac-x64.zip"
            ),
            OS::Windows => format!(
                "https://edgedl.me.gvt1.com/edgedl/chrome/chrome-for-testing/{}/{}/{}",
                version, "win64", "chromedriver-win64.zip"
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
    let body = resp.bytes().await?;

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(body))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = file.mangled_name();
        if file.name().ends_with('/') {
            tokio::fs::create_dir_all(&outpath).await?;
        } else {
            let outpath_relative = outpath.file_name().ok_or_else(|| {
                format!(
                    "Could not get file name from path: {}",
                    outpath.to_string_lossy()
                )
            })?;
            let mut outfile = std::fs::File::create(outpath_relative)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }
    Ok(())
}

async fn get_chrome_version(os: OS) -> Result<String, Box<dyn std::error::Error>> {
    log::info!("Getting installed Chrome version...");
    let command = match os {
        OS::Linux => {
            Command::new("/usr/bin/google-chrome")
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
