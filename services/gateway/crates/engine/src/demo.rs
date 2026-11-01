//! Deterministic synthetic retail data (hash-based: every run produces the same rows), shaped
//! like the design's "Acme Retail". Used by the demo seed and the engine benchmarks.

pub const STORES: &str = "SELECT i AS store_id,
       'Store ' || lpad(i::VARCHAR, 3, '0') AS store_name,
       (['AMER', 'EMEA', 'APAC'])[1 + i % 3] AS region,
       (['New York', 'London', 'Singapore', 'Chicago', 'Berlin', 'Tokyo',
         'Toronto', 'Paris', 'Sydney', 'São Paulo', 'Madrid', 'Mumbai'])[1 + i % 12] AS city,
       CASE WHEN i % 10 = 0 THEN 'online' ELSE 'retail' END AS store_type,
       DATE '2019-03-01' + CAST(i * 37 AS INTEGER) AS opened_on
FROM range(1, 61) t(i)";

pub fn customers_sql(customers: u64) -> String {
    format!(
        "SELECT i AS customer_id,
                'Customer ' || i AS customer_name,
                CASE WHEN hash(i) % 10 < 7 THEN 'consumer' ELSE 'business' END AS segment,
                (['AMER', 'EMEA', 'APAC'])[CAST(1 + hash(i * 3) % 3 AS INTEGER)] AS home_region,
                DATE '2021-01-01' + CAST(hash(i * 7) % 2000 AS INTEGER) AS signed_up_on,
                hash(i * 11) % 100 = 0 AS is_internal
         FROM range(1, {customers} + 1) t(i)"
    )
}

pub fn orders_sql(orders: u64, customers: u64) -> String {
    format!(
        "SELECT order_id, order_date, store_id,
                (['AMER', 'EMEA', 'APAC'])[1 + store_id % 3] AS region,
                CASE WHEN store_id % 10 = 0 THEN 'online' ELSE 'in store' END AS channel,
                customer_id, category, status, quantity, amount
         FROM (
            SELECT i AS order_id,
                   DATE '2024-02-01' + CAST(hash(i) % 1004 AS INTEGER) AS order_date,
                   CAST(1 + hash(i * 3) % 60 AS INTEGER) AS store_id,
                   CAST(1 + hash(i * 5) % {customers} AS BIGINT) AS customer_id,
                   (['Home & kitchen', 'Outdoor', 'Apparel', 'Electronics',
                     'Beauty', 'Toys', 'Grocery', 'Sports'])[CAST(1 + hash(i * 7) % 8 AS INTEGER)] AS category,
                   CASE WHEN hash(i * 11) % 100 < 95 THEN 'settled'
                        WHEN hash(i * 11) % 100 < 98 THEN 'refunded'
                        ELSE 'pending' END AS status,
                   CAST(1 + hash(i * 13) % 5 AS INTEGER) AS quantity,
                   CAST((5 + (hash(i * 17) % 19500) / 100.0) * (1 + hash(i * 13) % 5) AS DECIMAL(12, 2)) AS amount
            FROM range(1, {orders} + 1) t(i)
         )"
    )
}
