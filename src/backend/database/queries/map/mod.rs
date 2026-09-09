pub const TOTAL_CHANGES: &str = "SELECT total_changes()";
pub const LONGITUDE_CLAUSE_STANDARD: &str = "mm.gps_longitude BETWEEN ? AND ?";
pub const LONGITUDE_CLAUSE_ANTIMERIDIAN: &str = "(mm.gps_longitude >= ? OR mm.gps_longitude <= ?)";

pub fn build_clusters_query(precision_bits: usize) -> String {
    let full = precision_bits / 5;
    let partial = precision_bits % 5;
    let alphabet = crate::database::map::GEOHASH_ALPHABET;
    let cell = if partial == 0 {
        format!("SUBSTR(mm.geohash, 1, {full})")
    } else {
        let position = full + 1;
        let width = 1 << (5 - partial);
        format!("SUBSTR(mm.geohash, 1, {full}) || SUBSTR('{alphabet}', ((INSTR('{alphabet}', SUBSTR(mm.geohash, {position}, 1)) - 1) / {width}) * {width} + 1, 1) || ':{partial}'")
    };
    format!(
        r#"
        WITH clustered AS (
            SELECT {cell} AS cell
                 , COUNT(*) AS count
                 , AVG(mm.gps_latitude) AS center_lat
                 , AVG(mm.gps_longitude) AS center_lon
                 , MAX(COALESCE(mm.date_taken, m.created_at) || '_' || PRINTF('%020d', m.id)) AS latest
              FROM media AS m
              JOIN media_access AS ma ON m.id = ma.media_id
              JOIN media_metadata AS mm ON m.id = mm.media_id
             WHERE ma.user_id = ?
               AND ma.deleted_at IS NULL
               AND mm.gps_latitude <> 0
               AND mm.gps_longitude <> 0
               AND mm.geohash IS NOT NULL
               AND mm.gps_latitude BETWEEN -90 AND 90
               AND mm.gps_longitude BETWEEN -180 AND 180
              GROUP BY cell
        )
        SELECT c.cell
             , c.count
             , c.center_lat
             , c.center_lon
             , CAST(SUBSTR(c.latest, INSTR(c.latest, '_') + 1) AS INTEGER) AS representative_id
          FROM clustered AS c
         ORDER BY CAST(c.center_lat + 90 AS INTEGER), c.center_lon, c.cell
        "#,
    )
}

pub fn build_media_query(geohash_count: usize, longitude_clause: &str) -> String {
    let geohash_clause = if geohash_count > 0 {
        let conditions = (0..geohash_count)
            .map(|_| "mm.geohash GLOB ?")
            .collect::<Vec<_>>()
            .join(" OR ");
        format!("\n               AND ({})", conditions)
    } else {
        String::new()
    };

    format!(
        r#"
        SELECT {media_columns}
             , m.content_hash
             , m.created_at
          FROM media AS m
          JOIN media_access AS ma ON m.id = ma.media_id
          JOIN media_metadata AS mm ON m.id = mm.media_id
         WHERE ma.user_id = ?
           AND ma.deleted_at IS NULL
            AND (?2 = 1 OR (mm.gps_latitude BETWEEN ?3 AND ?4
            AND {longitude_clause}))
            AND mm.gps_latitude <> 0
            AND mm.gps_longitude <> 0
            AND mm.gps_latitude IS NOT NULL
           AND mm.gps_longitude IS NOT NULL
           AND mm.geohash IS NOT NULL{geohash_clause}
         ORDER BY COALESCE(mm.date_taken, m.created_at) DESC
                , m.id DESC
        "#,
        media_columns = media_response_columns!(),
        longitude_clause = longitude_clause,
        geohash_clause = geohash_clause
    )
}
