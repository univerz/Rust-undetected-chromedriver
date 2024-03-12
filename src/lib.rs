mod google_api;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::os::unix::fs::PermissionsExt;
use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use rand::Rng;
use thirtyfour::{ChromeCapabilities, DesiredCapabilities, WebDriver};
use tokio::process::{Child, Command};

use crate::google_api::fetch_chromedriver;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OS {
    Linux,
    MacOS,
    Windows,
}

#[derive(Debug)]
pub struct Driver {
    pub url: String,
    pub process: Child,
}

/// A wrapper around a WebDriver that holds an `Arc<Driver>`
/// When all `Arc<Driver>` are dropped, the chromedriver process is killed, this
/// ensures we're not leaking chromedriver processes and occupying ports.
pub struct UndetectedChrome {
    pub driver: Arc<Driver>,
    pub chrome: WebDriver,
}

impl UndetectedChrome {
    pub async fn quit(self) -> anyhow::Result<()> {
        self.chrome.quit().await?;
        Ok(())
    }
}

impl DerefMut for UndetectedChrome {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.chrome
    }
}

impl Deref for UndetectedChrome {
    type Target = WebDriver;

    fn deref(&self) -> &Self::Target {
        &self.chrome
    }
}

#[derive(Debug)]
pub struct ChromeBuilder {
    driver: Option<Arc<Driver>>,
    caps: Option<ChromeCapabilities>,
}

impl ChromeBuilder {
    pub fn new() -> Self {
        Self {
            driver: None,
            caps: None,
        }
    }

    pub fn with_driver(mut self, driver: Arc<Driver>) -> Self {
        self.driver = Some(driver);
        self
    }

    pub fn with_caps(mut self, caps: ChromeCapabilities) -> Self {
        self.caps = Some(caps);
        self
    }

    pub async fn build(self) -> anyhow::Result<UndetectedChrome> {
        let mut caps = self.caps.unwrap_or_else(|| DesiredCapabilities::chrome());
        let driver = match self.driver {
            Some(d) => d,
            None => Arc::new(start_driver().await?),
        };

        caps.set_no_sandbox().unwrap();
        caps.set_disable_dev_shm_usage().unwrap();
        caps.add_chrome_arg("--disable-blink-features=AutomationControlled")
            .unwrap();
        caps.add_chrome_arg("window-size=1920,1080").unwrap();
        caps.add_chrome_arg("user-agent=Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.6261.111 Safari/537.36").unwrap();
        caps.add_chrome_arg("disable-infobars").unwrap();
        caps.add_chrome_option("excludeSwitches", ["enable-automation"])
            .unwrap();
        let mut attempts = 0;
        let client = reqwest::Client::new();
        loop {
            attempts += 1;
            if client
                .get(&format!("{}/status", driver.url))
                .send()
                .await
                .is_ok()
            {
                break;
            }
            if attempts > 20 {
                anyhow::bail!("failed to connect to chromedriver");
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
        let chrome = WebDriver::new(&driver.url, caps.clone()).await?;
        Ok(UndetectedChrome { driver, chrome })
    }
}

fn random_char() -> u8 {
    let alphabet = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ".as_bytes();
    alphabet[rand::thread_rng().gen_range(0..48)]
}

/// Launches a new Chromedriver instance and returns a WebDriver running on it.
pub async fn chrome() -> anyhow::Result<UndetectedChrome> {
    ChromeBuilder::new().build().await
}

pub async fn start_driver() -> anyhow::Result<Driver> {
    let os = match std::env::consts::OS {
        "linux" => OS::Linux,
        "macos" => OS::MacOS,
        "windows" => OS::Windows,
        unknown_os => anyhow::bail!("unsupported OS: `{}`", unknown_os),
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

    let patched_chromedriver_path = match os {
        OS::Linux | OS::MacOS => "chromedriver_PATCHED",
        OS::Windows => "chromedriver_PATCHED.exe",
    };

    if !tokio::fs::try_exists(patched_chromedriver_path).await? {
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
        let mut patch_ct = -1;
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
            patched_chromedriver_path
        );
        tokio::fs::write(patched_chromedriver_path, new_chromedriver_bytes).await?;
        log::info!(
            "Successfully wrote patched executable to {}",
            patched_chromedriver_path
        );
    } else {
        log::info!("Detected patched chromedriver executable!");
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let mut perms = tokio::fs::metadata(patched_chromedriver_path)
            .await?
            .permissions();
        perms.set_mode(0o755);
        tokio::fs::set_permissions(patched_chromedriver_path, perms).await?;
    }

    log::info!("Starting chromedriver...");
    let port: usize = rand::thread_rng().gen_range(2000..5000);
    let url = format!("http://localhost:{}", port);
    let process = Command::new(format!("./{}", patched_chromedriver_path))
        .arg(format!("--port={}", port))
        .kill_on_drop(true)
        .spawn()?;
    Ok(Driver { url, process })
}
