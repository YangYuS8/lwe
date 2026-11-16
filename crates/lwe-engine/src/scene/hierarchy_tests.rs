fn check_hierarchy_pair(actual: [f32; 2], expected: [f32; 2]) {
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(a, b)| (a - b).abs() < 2e-5),
        "expected {expected:?}, got {actual:?}"
    );
}

#[test]
fn hierarchy_resolves_nested_local_transforms_without_reordering_draws() {
    let fixture = Fixture::new();
    fixture.scene(serde_json::json!([
        {"id":70,"name":"Pivot","origin":"8 7 0","angles":"0 0 1.57079632679","scale":"2 2 1"},
        {"id":11,"image":"models/image.json","parent":70,"origin":"2 -1 0",
         "angles":"0 0 -1.57079632679","size":"3 5"},
        {"id":95,"image":"models/image.json","origin":"10 11 0","size":"5 3"},
        {"id":5,"image":"models/image.json","parent":70,"origin":"-1 2 0","size":"7 1"},
        {"id":90,"image":"models/image.json","parent":11,"origin":"1 0 0","size":"1 3"},
        {"id":51,"image":"models/image.json","parent":70,"origin":"0 0 0",
         "visible":false,"size":"9 9"}
    ]));
    let scene = PreparedScene::prepare(&fixture.0, None).unwrap();
    assert_eq!(scene.summary.layer_count, 4);
    assert_eq!(scene.summary.pass_count, 4);
    assert!(!scene.summary.animated);
    // Use authored dimensions as markers: the root between siblings stays between
    // them, even though it is not part of their transform tree.
    let layers: Vec<_> = scene
        .draw_order
        .iter()
        .map(|object| match object {
            DrawObject::Image(index) => &scene.layers[*index],
            DrawObject::Particles(_) => panic!("hierarchy fixture contains no particles"),
        })
        .collect();
    assert_eq!(
        layers.iter().map(|layer| layer.size).collect::<Vec<_>>(),
        [[3., 5.], [5., 3.], [7., 1.], [1., 3.]]
    );
    assert!(layers[1].world_2d.is_none());

    let child = layers[0].world_2d.as_ref().expect("child world transform");
    // A quarter turn sends the scaled (4,-2) offset to (2,4).
    check_hierarchy_pair(child.origin, [10., 11.]);
    // The counter-rotation cancels the parent's turn, but not its scale.
    check_hierarchy_pair(child.x, [2., 0.]);
    check_hierarchy_pair(child.y, [0., 2.]);
    let sibling = layers[2]
        .world_2d
        .as_ref()
        .expect("sibling world transform");
    check_hierarchy_pair(sibling.origin, [4., 5.]);
    check_hierarchy_pair(sibling.x, [0., 2.]);
    check_hierarchy_pair(sibling.y, [-2., 0.]);
    let grandchild = layers[3]
        .world_2d
        .as_ref()
        .expect("grandchild world transform");
    check_hierarchy_pair(grandchild.origin, [12., 11.]);
    check_hierarchy_pair(grandchild.x, [2., 0.]);
    check_hierarchy_pair(grandchild.y, [0., 2.]);
}

#[test]
fn hierarchy_composes_shear_without_changing_authored_dimensions() {
    let fixture = Fixture::new();
    fixture.scene(serde_json::json!([
        {"id":1,"image":"models/image.json","origin":"8 8 0","size":"7 9","scale":"2 1 1"},
        {"id":2,"image":"models/image.json","parent":1,"origin":"2 1 0","size":"3 5",
         "angles":"0 0 0.785398163397"}
    ]));
    let scene = PreparedScene::prepare(&fixture.0, None).unwrap();
    assert_eq!(scene.summary.layer_count, 2);
    assert!(matches!(
        scene.draw_order.as_slice(),
        [DrawObject::Image(0), DrawObject::Image(1)]
    ));
    assert_eq!(scene.layers[0].size, [7., 9.]);
    assert_eq!(scene.layers[1].size, [3., 5.]);
    let child = scene.layers[1].world_2d.as_ref().unwrap();
    check_hierarchy_pair(child.origin, [12., 9.]);
    // The column dot product is -1.5, so an orthogonal rotate+scale
    // decomposition (or scaling the authored dimensions) loses information.
    let c = std::f32::consts::FRAC_1_SQRT_2;
    check_hierarchy_pair(child.x, [2. * c, c]);
    check_hierarchy_pair(child.y, [-2. * c, c]);
}

fn expect_hierarchy_rejection(fixture: &Fixture, parent: Value, child: Value, label: &str) {
    fixture.scene(serde_json::json!([parent, child]));
    match PreparedScene::prepare(&fixture.0, None) {
        Ok(_) => panic!("{label}: invalid hierarchy prepared"),
        Err(error) => assert_eq!(
            unsupported_feature_code(&error),
            Some("hierarchy"),
            "{label}: {error:#}"
        ),
    }
}

#[test]
fn hierarchy_rejects_invalid_ids_and_edges_including_hidden_leaves() {
    let fixture = Fixture::new();
    let parent = serde_json::json!({"id":1,"name":"Pivot","origin":"8 8 0"});
    let child = serde_json::json!({"id":2,"image":"models/image.json","origin":"1 0 0","parent":1});
    for (label, id) in [
        ("missing target", serde_json::json!(17)),
        ("self-parent", serde_json::json!(2)),
        ("zero parent", serde_json::json!(0)),
        ("negative parent", serde_json::json!(-1)),
        ("fractional parent", serde_json::json!(1.25)),
        ("string parent", serde_json::json!("1")),
        ("null parent", Value::Null),
    ] {
        let mut invalid = child.clone();
        invalid["parent"] = id;
        invalid["visible"] = Value::Bool(false);
        expect_hierarchy_rejection(&fixture, parent.clone(), invalid, label);
    }

    let mut duplicate = child.clone();
    duplicate["id"] = serde_json::json!(1);
    expect_hierarchy_rejection(&fixture, parent.clone(), duplicate, "duplicate id");
    let mut invalid_id = child.clone();
    invalid_id["id"] = serde_json::json!(-2);
    expect_hierarchy_rejection(&fixture, parent.clone(), invalid_id, "invalid child id");
    let mut cycle = parent;
    cycle["parent"] = serde_json::json!(2);
    expect_hierarchy_rejection(&fixture, cycle, child, "two-node cycle");
}

#[test]
fn hierarchy_rejects_unsupported_transforms_attachments_and_visibility() {
    let fixture = Fixture::new();
    let parent = serde_json::json!({"id":1,"name":"Pivot","origin":"8 8 0"});
    let child = serde_json::json!({"id":2,"image":"models/image.json","origin":"1 0 0","parent":1});
    for (label, field, value) in [
        ("out-of-plane origin", "origin", serde_json::json!("1 0 1")),
        ("out-of-plane angle", "angles", serde_json::json!("0.5 0 0")),
        ("attachment", "attachment", serde_json::json!("bone")),
        (
            "script-controlled child",
            "origin",
            serde_json::json!({
                "value":"1 0 0",
                "script":"export function update(value){return value;}"
            }),
        ),
    ] {
        let mut invalid = child.clone();
        invalid[field] = value;
        expect_hierarchy_rejection(&fixture, parent.clone(), invalid, label);
    }
    let mut missing_origin = child.clone();
    missing_origin.as_object_mut().unwrap().remove("origin");
    expect_hierarchy_rejection(
        &fixture,
        parent.clone(),
        missing_origin,
        "missing child-local origin",
    );
    let mut hidden_parent = parent.clone();
    hidden_parent["visible"] = Value::Bool(false);
    expect_hierarchy_rejection(&fixture, hidden_parent, child.clone(), "hidden parent");
    fixture.write(
        "project.json",
        br#"{"type":"scene","file":"scene.json","general":{"properties":{"show":{"type":"bool","value":true}}}}"#,
    );
    let mut property_parent = parent;
    property_parent["visible"] = serde_json::json!({"user":"show","value":true});
    fixture.scene(serde_json::json!([property_parent, child]));
    let shown = PreparedScene::prepare(&fixture.0, None).unwrap();
    assert_eq!(shown.summary.layer_count, 1);
    let hidden = BTreeMap::from([("show".to_owned(), Value::Bool(false))]);
    match PreparedScene::prepare_with_properties(&fixture.0, None, &hidden) {
        Ok(_) => panic!("property override wrongly admitted a hidden parent"),
        Err(error) => assert_eq!(
            unsupported_feature_code(&error),
            Some("hierarchy"),
            "{error:#}"
        ),
    }
}
