<!-- i18n: language-switcher -->
[English](testing.md) | [日本語](testing.ja.md)

# テスト

デフォルトゲート：

```sh
cargo fmt -- --check
cargo test
cargo test --all-features
cargo clippy --all-features --all-targets -- -D warnings
```

コンテナSQLスモークテスト：

```sh
cargo test --test container_sql -- --ignored --test-threads=1
```

外部管理のPostgres/MySQLスモークテスト：

```sh
export IRODORI_POSTGRES_URL='postgres://postgres:postgres@127.0.0.1:5432/irodori_migration'
export IRODORI_MYSQL_HOST='127.0.0.1'
export IRODORI_MYSQL_PORT='3306'
export IRODORI_MYSQL_USER='root'
export IRODORI_MYSQL_PASSWORD='mysql'
export IRODORI_MYSQL_DATABASE='irodori_migration'
cargo test --test live_sql -- --ignored --test-threads=1
```

パッケージチェック：

```sh
rm -f Cargo.lock
cargo package --list --allow-dirty
cargo publish --dry-run --allow-dirty
```