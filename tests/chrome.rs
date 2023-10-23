#[cfg(test)]
mod tests {
    use test_log::test;
    use undetected_chromedriver::chrome;

    #[test(tokio::test)]
    async fn test_chrome() {
        let driver = chrome().await.unwrap();
        assert!(driver.title().await.is_ok());
        driver.quit().await.unwrap();
    }
}
