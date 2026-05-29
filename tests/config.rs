use deskagent::config::AppConfig;

#[test]
fn config_round_trips_through_toml() {
    let config = AppConfig {
        provider: "openai".into(),
        api_key: Some("abc".into()),
    };
    let text = toml::to_string(&config).unwrap();
    let parsed: AppConfig = toml::from_str(&text).unwrap();
    assert_eq!(parsed.provider, "openai");
    assert_eq!(parsed.api_key.as_deref(), Some("abc"));
}
