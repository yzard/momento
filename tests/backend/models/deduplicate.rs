use momento_api::models::{DeduplicateGroup, DeduplicateGroupsResponse};

#[test]
fn groups_response_uses_frontend_contract_field_names() {
    let response = DeduplicateGroupsResponse {
        groups: vec![DeduplicateGroup {
            cluster_id: 7,
            items: Vec::new(),
        }],
        next_cursor: Some("7".to_string()),
        has_more: true,
        total_groups: 21,
        total_media: 34,
    };

    let json = serde_json::to_value(response).expect("Response should serialize");

    assert_eq!(json["groups"][0]["clusterId"], 7);
    assert_eq!(json["nextCursor"], "7");
    assert_eq!(json["hasMore"], true);
    assert_eq!(json["totalGroups"], 21);
    assert_eq!(json["totalMedia"], 34);
}
