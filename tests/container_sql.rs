use std::error::Error;

use irodori_migration::{
    chunk_checksum_select_sql, row_hash_select_sql, CanonicalColumn, CanonicalType,
    ChecksumAggregate, ChecksumFunction, ChunkBounds, ChunkChecksumConfig, MigrationEngine,
    MigrationSpec,
};
use mysql::prelude::Queryable;
use testcontainers_modules::{
    mysql::Mysql, postgres::Postgres, testcontainers::runners::SyncRunner,
};

type TestResult = Result<(), Box<dyn Error + Send + Sync + 'static>>;

#[test]
#[ignore = "requires Docker; run in CI with --ignored --test-threads=1"]
fn postgres_chunk_checksum_sql_executes_in_container() -> TestResult {
    let node = Postgres::default().start()?;
    let connection_string = format!(
        "postgres://postgres:postgres@{}:{}/postgres",
        node.get_host()?,
        node.get_host_port_ipv4(5432)?
    );
    let mut client = postgres::Client::connect(&connection_string, postgres::NoTls)?;

    client.batch_execute(
        "
        CREATE TABLE public.irodori_container_orders (
          order_id BIGINT PRIMARY KEY,
          amount NUMERIC(12,2),
          status TEXT
        );
        INSERT INTO public.irodori_container_orders(order_id, amount, status)
        VALUES (1, 10.25, 'paid'), (2, 0.00, NULL), (3, 7.50, 'open');
        ",
    )?;

    let sql = chunk_checksum_select_sql(
        MigrationEngine::Postgres,
        &ChunkChecksumConfig::new(
            "public.irodori_container_orders",
            vec![
                CanonicalColumn::new("order_id", CanonicalType::Integer),
                CanonicalColumn::new("amount", CanonicalType::Decimal { scale: 2 }),
                CanonicalColumn::new("status", CanonicalType::Text),
            ],
        )
        .with_bounds(ChunkBounds {
            column: "order_id".to_string(),
            lower: Some("1".to_string()),
            upper: Some("4".to_string()),
            include_upper: false,
        })
        .with_function(ChecksumFunction::Md5)
        .with_aggregate(ChecksumAggregate::Sum),
    );

    client.simple_query(&sql)?;
    Ok(())
}

#[test]
#[ignore = "requires Docker; run in CI with --ignored --test-threads=1"]
fn mysql_chunk_checksum_sql_executes_in_container() -> TestResult {
    let node = Mysql::default().start()?;
    let connection_string = format!(
        "mysql://root@{}:{}/test",
        node.get_host()?,
        node.get_host_port_ipv4(3306)?
    );
    let mut conn = mysql::Conn::new(mysql::Opts::from_url(&connection_string)?)?;

    conn.query_drop(
        "CREATE TABLE irodori_container_orders (
          order_id BIGINT PRIMARY KEY,
          amount DECIMAL(12,2),
          status VARCHAR(32)
        )",
    )?;
    conn.query_drop(
        "
        INSERT INTO irodori_container_orders(order_id, amount, status)
        VALUES (1, 10.25, 'paid'), (2, 0.00, NULL), (3, 7.50, 'open')
        ",
    )?;

    let sql = chunk_checksum_select_sql(
        MigrationEngine::MySql,
        &ChunkChecksumConfig::new(
            "irodori_container_orders",
            vec![
                CanonicalColumn::new("order_id", CanonicalType::Integer),
                CanonicalColumn::new("amount", CanonicalType::Decimal { scale: 2 }),
                CanonicalColumn::new("status", CanonicalType::Text),
            ],
        )
        .with_bounds(ChunkBounds {
            column: "order_id".to_string(),
            lower: Some("1".to_string()),
            upper: Some("4".to_string()),
            include_upper: false,
        })
        .with_function(ChecksumFunction::Crc32)
        .with_aggregate(ChecksumAggregate::BitXor),
    );

    conn.query_drop(sql)?;
    Ok(())
}

#[test]
#[ignore = "requires Docker; run in CI with --ignored --test-threads=1"]
fn postgres_and_mysql_row_hashes_match_for_same_data() -> TestResult {
    let postgres_node = Postgres::default().start()?;
    let postgres_url = format!(
        "postgres://postgres:postgres@{}:{}/postgres",
        postgres_node.get_host()?,
        postgres_node.get_host_port_ipv4(5432)?
    );
    let mut postgres = postgres::Client::connect(&postgres_url, postgres::NoTls)?;
    postgres.batch_execute(
        "
        CREATE TABLE public.irodori_hash_equivalence (
          order_id BIGINT PRIMARY KEY,
          amount NUMERIC(12,2),
          status TEXT
        );
        INSERT INTO public.irodori_hash_equivalence(order_id, amount, status)
        VALUES (1, 10.25, 'paid'), (2, 0.00, NULL), (3, 7.50, 'open');
        ",
    )?;

    let mysql_node = Mysql::default().start()?;
    let mysql_url = format!(
        "mysql://root@{}:{}/test",
        mysql_node.get_host()?,
        mysql_node.get_host_port_ipv4(3306)?
    );
    let mut mysql = mysql::Conn::new(mysql::Opts::from_url(&mysql_url)?)?;
    mysql.query_drop(
        "CREATE TABLE irodori_hash_equivalence (
          order_id BIGINT PRIMARY KEY,
          amount DECIMAL(12,2),
          status VARCHAR(32)
        )",
    )?;
    mysql.query_drop(
        "
        INSERT INTO irodori_hash_equivalence(order_id, amount, status)
        VALUES (1, 10.25, 'paid'), (2, 0.00, NULL), (3, 7.50, 'open')
        ",
    )?;

    let spec = MigrationSpec {
        source_engine: MigrationEngine::Postgres,
        target_engine: MigrationEngine::MySql,
        key_columns: vec!["order_id".to_string()],
        compare_columns: vec![
            "order_id".to_string(),
            "amount".to_string(),
            "status".to_string(),
        ],
        normalize_whitespace: true,
        ..MigrationSpec::default()
    };
    let columns = spec.compare_columns.clone();
    let keys = spec.key_columns.clone();
    let postgres_sql = format!(
        "{} ORDER BY order_id",
        row_hash_select_sql(
            MigrationEngine::Postgres,
            "public.irodori_hash_equivalence",
            &keys,
            &columns,
            "",
            &spec,
        )
    );
    let mysql_sql = format!(
        "{} ORDER BY order_id",
        row_hash_select_sql(
            MigrationEngine::MySql,
            "irodori_hash_equivalence",
            &keys,
            &columns,
            "",
            &spec,
        )
    );

    let postgres_hashes = postgres
        .query(&postgres_sql, &[])?
        .into_iter()
        .map(|row| row.get::<_, String>("irodori_row_hash"))
        .collect::<Vec<_>>();
    let mysql_hashes = mysql.query_map(mysql_sql, |row: mysql::Row| {
        row.get::<String, _>("irodori_row_hash")
            .expect("irodori_row_hash")
    })?;

    assert_eq!(postgres_hashes, mysql_hashes);
    Ok(())
}
