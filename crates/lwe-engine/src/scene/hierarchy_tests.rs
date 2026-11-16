fn check_hierarchy_pair(actual: [f32; 2], expected: [f32; 2]) {
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(a, b)| (a - b).abs() < 2e-5),
        "expected {expected:?}, got {actual:?}"
    );
}

fn initial_hierarchy_layer(scene: &PreparedScene, authored_index: usize) -> &hierarchy::Resolved {
    scene.hierarchy.as_ref().unwrap().initial[authored_index]
        .as_ref()
        .unwrap()
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
        {"id":51,"image":"models/unused-hidden-leaf.json","parent":70,"origin":"0 0 0",
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
    assert!(!initial_hierarchy_layer(&scene, 5).visible);

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
fn hierarchy_rejects_unsupported_transforms_and_attachments() {
    let fixture = Fixture::new();
    let parent = serde_json::json!({"id":1,"name":"Pivot","origin":"8 8 0"});
    let child = serde_json::json!({"id":2,"image":"models/image.json","origin":"1 0 0","parent":1});
    for (label, field, value) in [
        ("out-of-plane origin", "origin", serde_json::json!("1 0 1")),
        ("out-of-plane angle", "angles", serde_json::json!("0.5 0 0")),
        ("attachment", "attachment", serde_json::json!("bone")),
        (
            "scripted depth",
            "origin",
            serde_json::json!({
                "value":"1 0 0",
                "script":"export function update(){return new Vec3(1,0,1);}"
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
        parent,
        missing_origin,
        "missing child-local origin",
    );
}

#[test]
fn hierarchy_visibility_propagates_through_a_prepared_nested_chain() {
    let fixture = Fixture::new();
    let mut objects = serde_json::json!([
        {"id":1,"name":"Pivot","origin":"7 8 0","visible":false},
        {"id":2,"image":"models/image.json","parent":1,"origin":"1 0 0","visible":true},
        {"id":3,"image":"models/image.json","parent":2,"origin":"2 1 0","visible":true},
        {"id":4,"image":"models/image.json","origin":"11 10 0"}
    ]);
    fixture.scene(objects.clone());
    let literal_hidden = PreparedScene::prepare(&fixture.0, None).unwrap();
    assert_eq!(literal_hidden.summary.layer_count, 3);
    assert_eq!(literal_hidden.summary.pass_count, 3);
    assert_eq!(
        literal_hidden.hierarchy.as_ref().unwrap().layers,
        [(1, 0), (2, 1)]
    );
    assert!(literal_hidden.layers[2].world_2d.is_none());
    assert!(!initial_hierarchy_layer(&literal_hidden, 1).visible);
    let grandchild = initial_hierarchy_layer(&literal_hidden, 2);
    assert!(!grandchild.visible);
    check_hierarchy_pair(grandchild.world.origin, [10., 9.]);

    fixture.write(
        "project.json",
        br#"{"type":"scene","file":"scene.json","general":{"properties":{"show":{"type":"bool","value":true}}}}"#,
    );
    objects[0]["visible"] = serde_json::json!({"user":"show","value":true});
    fixture.scene(objects.clone());
    let shown = PreparedScene::prepare(&fixture.0, None).unwrap();
    let hide = BTreeMap::from([("show".to_owned(), Value::Bool(false))]);
    let hidden = PreparedScene::prepare_with_properties(&fixture.0, None, &hide).unwrap();
    assert_eq!(shown.summary, hidden.summary);
    for authored_index in [1, 2] {
        assert!(initial_hierarchy_layer(&shown, authored_index).visible);
        assert!(!initial_hierarchy_layer(&hidden, authored_index).visible);
    }

    // Effective visibility must not let an unvalidated asset into the graph.
    objects[2]["image"] = serde_json::json!("models/hidden-invalid.json");
    fixture.write("models/hidden-invalid.json", b"{");
    fixture.scene(objects);
    match PreparedScene::prepare_with_properties(&fixture.0, None, &hide) {
        Ok(_) => panic!("hidden grandchild's invalid model was not validated"),
        Err(error) => assert!(format!("{error:#}").contains("models/hidden-invalid.json")),
    }
}

#[test]
fn hierarchy_resolves_scripted_translation_and_visibility_from_frame_packets() {
    let fixture = Fixture::new();
    fixture.scene(serde_json::json!([
        {"id":10,"name":"Pivot","scale":"2 2 1",
         "origin":{"value":"2 3 0",
          "script":"export function update(){return new Vec3(4+engine.runtime,5,0);}"},
         "visible":{"value":true,
          "script":"export function update(){return engine.runtime<0.5||engine.runtime>=1.5;}"}},
        {"id":20,"image":"models/image.json","parent":10,
         "origin":{"value":"2 1 0",
          "script":"export function update(){return new Vec3(2,1+engine.runtime,0);}"}},
        {"id":30,"image":"models/image.json","parent":20,"origin":"1 2 0"}
    ]));
    let scene = PreparedScene::prepare(&fixture.0, None).unwrap();
    assert!(scene.summary.animated);
    assert_eq!(scene.summary.layer_count, 2);
    let plan = scene.hierarchy.as_ref().unwrap();
    assert_eq!(plan.layers, [(1, 0), (2, 1)]);
    let initial = initial_hierarchy_layer(&scene, 2);
    assert!(initial.visible);
    check_hierarchy_pair(initial.world.origin, [10., 11.]);

    let worker = dynamic_script::ScriptWorker::spawn(scene.scripts.as_ref().unwrap()).unwrap();
    let take = || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            if let Some(packet) = worker.take_frame().unwrap() {
                break packet;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "script worker timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    };
    let first = take();
    assert_eq!((first.seq, first.time), (0, 0.));
    for (seq, visible, child_origin) in [(1, false, [11., 13.]), (2, true, [12., 15.])] {
        let now = seq as f64;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !worker
            .submit_frame(seq, now, spectrum::SpectrumSnapshot::default())
            .unwrap()
        {
            assert!(
                std::time::Instant::now() < deadline,
                "script submission timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let frame = take();
        assert_eq!((frame.seq, frame.time), (seq, now));
        assert!(
            frame
                .states
                .iter()
                .find(|state| state.handle == 3)
                .unwrap()
                .visible
        );
        let resolved = plan.graph.resolve(&frame.states).unwrap();
        assert_eq!(resolved[1].as_ref().unwrap().visible, visible);
        let grandchild = resolved[2].as_ref().unwrap();
        assert_eq!(grandchild.visible, visible);
        check_hierarchy_pair(grandchild.world.origin, child_origin);
    }
}
