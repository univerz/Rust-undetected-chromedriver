#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use test_log::test;
    use undetected_chromedriver::{chrome, start_driver, ChromeBuilder};

    #[test(tokio::test)]
    async fn test_chrome() {
        let driver = chrome().await.unwrap();
        assert!(driver.title().await.is_ok());
        driver.quit().await.unwrap();
    }
    #[test(tokio::test)]
    async fn test_two_chrome_one_driver() -> anyhow::Result<()> {
        let driver = Arc::new(start_driver().await?);
        let chrome_1 = ChromeBuilder::new()
            .with_driver(driver.clone())
            .build()
            .await?;
        let chrome_2 = ChromeBuilder::new()
            .with_driver(driver.clone())
            .build()
            .await?;
        Ok(())
    }
}
