use anyhow::{Result, ensure};
use serde_json::{Map, Value};

const MAX_KEYS: usize = 128;
const MAX_LENGTH: u64 = 4096;
const MAX_FPS: f64 = 240.0;
const MAX_TEXT_BYTES: usize = 256;
// Leave room for a sized quad, the largest cover ratio of positive i32
// dimensions, and vertex sums when a sampled value drives a transform.
const MAX_VECTOR_COMPONENT: f32 = f32::MAX / (16.0 * 16384.0 * i32::MAX as f32);

#[derive(Debug, Clone)]
pub(super) struct PropertyTimeline {
    channels: Channels,
    pub clock: ClockSpec,
    pub parent: Option<String>,
    pub children: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ClockSpec {
    length: u32,
    duration: f32,
    frame_seconds: f32,
    pub start_paused: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TimelinePosition {
    frame: u32,
    phase: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Sample {
    Scalar(f32),
    Vector([f32; 3]),
}

#[derive(Debug, Clone)]
enum Channels {
    Scalar(Curve),
    Vector([Curve; 3]),
}

#[derive(Debug, Clone)]
struct Curve {
    keys: Vec<Key>,
}

#[derive(Debug, Clone, Copy)]
struct Key {
    frame: u32,
    value: f32,
    incoming: [f32; 2],
    outgoing: [f32; 2],
    step: bool,
}

impl PropertyTimeline {
    pub(super) fn parse(animation: &Value, property: &str, base: &Value) -> Result<Self> {
        let animation = fields(Some(animation), &["c0", "c1", "c2", "options", "relative"])?;
        let options = fields(
            animation.get("options"),
            &[
                "fps",
                "length",
                "mode",
                "random",
                "startpaused",
                "wraploop",
                "name",
                "parent",
                "children",
            ],
        )?;
        let clock = ClockSpec::parse(options)?;
        if let Some(name) = options.get("name") {
            ensure!(
                name.as_str().is_some_and(
                    |name| name.len() <= MAX_TEXT_BYTES && !name.chars().any(char::is_control)
                ),
                invalid("timeline name is not bounded text")
            );
        }
        let parent = options.get("parent").map(link_key).transpose()?;
        let children = if let Some(children) = options.get("children") {
            children
                .as_array()
                .filter(|children| children.len() <= super::MAX_LAYERS)
                .ok_or_else(|| invalid("timeline children exceed their budget"))?
                .iter()
                .map(link_key)
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        let mut remaining_keys = MAX_KEYS;
        let channels = match property {
            "alpha" => {
                ensure!(
                    !["relative", "c1", "c2"]
                        .iter()
                        .any(|field| animation.contains_key(*field)),
                    invalid("alpha timeline requires one absolute channel")
                );
                Channels::Scalar(Curve::parse(
                    animation.get("c0"),
                    clock.length,
                    None,
                    &mut remaining_keys,
                )?)
            }
            "origin" | "scale" => {
                ensure!(
                    animation.get("relative").and_then(Value::as_bool) == Some(true),
                    invalid("vector timeline requires the authored relative flag")
                );
                let base = relative_base(base)?;
                let mut read = |key, component| {
                    Curve::parse(
                        animation.get(key),
                        clock.length,
                        Some(component),
                        &mut remaining_keys,
                    )
                };
                let curves = [
                    read("c0", base[0])?,
                    read("c1", base[1])?,
                    read("c2", base[2])?,
                ];
                if property == "origin" {
                    ensure!(
                        curves[2].keys.iter().all(|key| key.value == base[2]
                            && key.incoming[1] == 0.0
                            && key.outgoing[1] == 0.0),
                        invalid("relative origin timeline cannot change Scene depth")
                    );
                }
                Channels::Vector(curves)
            }
            _ => return Err(invalid("unsupported layer property timeline")),
        };
        Ok(Self {
            channels,
            clock,
            parent,
            children,
        })
    }

    pub(super) fn sample(&self, position: TimelinePosition) -> Sample {
        match &self.channels {
            Channels::Scalar(curve) => Sample::Scalar(curve.sample(position).clamp(0.0, 1.0)),
            Channels::Vector(curves) => {
                Sample::Vector(std::array::from_fn(|i| curves[i].sample(position)))
            }
        }
    }
}

impl ClockSpec {
    fn parse(options: &Map<String, Value>) -> Result<Self> {
        ensure!(
            options.get("mode").and_then(Value::as_str) == Some("single")
                && !flag(options.get("random"))?
                && !flag(options.get("wraploop").filter(|value| !value.is_null()))?,
            invalid("only deterministic single playback is supported")
        );
        let start_paused = flag(options.get("startpaused"))?;
        let fps = options
            .get("fps")
            .and_then(Value::as_f64)
            .filter(|fps| (1.0..=MAX_FPS).contains(fps))
            .ok_or_else(|| invalid("frame rate is outside the supported range"))?
            as f32;
        let length = options
            .get("length")
            .and_then(Value::as_u64)
            .filter(|length| (1..=MAX_LENGTH).contains(length))
            .ok_or_else(|| invalid("frame count is outside the supported range"))?
            as u32;
        Ok(Self {
            length,
            duration: length as f32 / fps,
            frame_seconds: 1.0 / fps,
            start_paused,
        })
    }

    pub(super) fn position(&self, time: f64) -> TimelinePosition {
        // The owner chooses time zero for a paused track. A running track
        // holds the single-play duration when it finishes.
        let time = (time.max(0.0) as f32).min(self.duration);
        let frame = ((time / self.frame_seconds) as u32).min(self.length - 1);
        let phase = (time % self.frame_seconds) / self.frame_seconds;
        TimelinePosition { frame, phase }
    }

    pub(super) fn finished(&self, elapsed: f64) -> bool {
        elapsed as f32 >= self.duration
    }
}

impl Curve {
    fn parse(
        channel: Option<&Value>,
        length: u32,
        relative_base: Option<f32>,
        remaining_keys: &mut usize,
    ) -> Result<Self> {
        let channel = channel
            .and_then(Value::as_array)
            .filter(|keys| (1..=*remaining_keys).contains(&keys.len()))
            .ok_or_else(|| invalid("curve keys are missing or exceed their budget"))?;
        *remaining_keys -= channel.len();
        let (min, max) = if relative_base.is_some() {
            (-MAX_VECTOR_COMPONENT, MAX_VECTOR_COMPONENT)
        } else {
            (0.0, 1.0)
        };
        let mut keys = Vec::<Key>::with_capacity(channel.len());
        for key in channel {
            let key = fields(
                Some(key),
                &[
                    "frame",
                    "value",
                    "back",
                    "front",
                    "lockangle",
                    "locklength",
                    "step",
                ],
            )?;
            flag(key.get("lockangle"))?;
            flag(key.get("locklength"))?;
            let frame = key
                .get("frame")
                .and_then(Value::as_u64)
                .filter(|frame| *frame < u64::from(length))
                .ok_or_else(|| invalid("key frame is outside the timeline"))?
                as u32;
            // The native loader adds each authored base component to the
            // channel's keys in f32, before building its curve.
            let value = bounded_number(key.get("value"), f64::from(min), f64::from(max))?
                + relative_base.unwrap_or(0.0);
            ensure!(
                (min..=max).contains(&value),
                invalid("key value exceeds the supported range")
            );
            let key = Key {
                frame,
                value,
                incoming: handle(key.get("back"), -1.0, 0.0, f64::from(max))?,
                outgoing: handle(key.get("front"), 0.0, 1.0, f64::from(max))?,
                step: flag(key.get("step"))?,
            };
            if let Some(previous) = keys.last() {
                ensure!(
                    previous.frame < key.frame,
                    invalid("keys must be strictly ordered")
                );
                // The convex control hull bounds every interpolated value.
                if !key.step {
                    ensure!(
                        (min..=max).contains(&(previous.value + previous.outgoing[1]))
                            && (min..=max).contains(&(key.value + key.incoming[1])),
                        invalid("curve controls exceed the supported range")
                    );
                }
            }
            keys.push(key);
        }
        Ok(Self { keys })
    }

    fn sample(&self, position: TimelinePosition) -> f32 {
        // The owner's integral frames and fractional phase also drive every
        // child in that clock group, even when its own author options differ.
        let left = self.at_frame(position.frame);
        let right = self.at_frame(position.frame + 1);
        if left == right {
            // A constant channel, including a planar layer's depth, must not
            // drift by a rounding unit when resampled.
            return left;
        }
        left * (1.0 - position.phase) + right * position.phase
    }

    fn at_frame(&self, frame: u32) -> f32 {
        let next = self.keys.partition_point(|key| key.frame <= frame);
        let [left, right] = match next {
            0 => return self.keys[0].value,
            next if next == self.keys.len() => return self.keys[next - 1].value,
            next => [self.keys[next - 1], self.keys[next]],
        };
        if frame == left.frame
            || right.step
            || (left.value == right.value && left.outgoing[1] == 0.0 && right.incoming[1] == 0.0)
        {
            return left.value;
        }

        let half_span = (right.frame - left.frame) as f32 * 0.5;
        let x0 = left.frame as f32;
        let x3 = right.frame as f32;
        let x = [
            x0,
            x0 + half_span * left.outgoing[0],
            x3 + half_span * right.incoming[0],
            x3,
        ];
        let target = frame as f32;
        // Match the native integer-frame inversion: the initial integer
        // quotient is zero inside each interval, with a .01-frame tolerance.
        let mut t = 0.0;
        let mut step = 0.999;
        for _ in 0..1000 {
            let position = cubic(x, t);
            if f64::from((position - target).abs()) < 0.01 {
                break;
            }
            step *= 0.5;
            if position > target {
                t -= step;
            } else {
                t += step;
            }
        }
        cubic(
            [
                left.value,
                left.value + left.outgoing[1],
                right.value + right.incoming[1],
                right.value,
            ],
            t.clamp(0.0, 1.0),
        )
    }
}

fn cubic([a, b, c, d]: [f32; 4], t: f32) -> f32 {
    let u = 1.0 - t;
    let u3 = u * 3.0;
    (u3 * u * t) * b + (u * u * u) * a + (u3 * t * t) * c + (t * t * t) * d
}

fn invalid(detail: &'static str) -> anyhow::Error {
    super::unsupported("timeline", detail)
}

fn fields<'a>(value: Option<&'a Value>, allowed: &[&str]) -> Result<&'a Map<String, Value>> {
    value
        .and_then(Value::as_object)
        .filter(|object| object.keys().all(|key| allowed.contains(&key.as_str())))
        .ok_or_else(|| invalid("unrecognized timeline fields"))
}

fn flag(value: Option<&Value>) -> Result<bool> {
    value.map_or(Ok(false), |value| {
        value
            .as_bool()
            .ok_or_else(|| invalid("timeline flags must be booleans"))
    })
}

fn bounded_number(value: Option<&Value>, low: f64, high: f64) -> Result<f32> {
    value
        .and_then(Value::as_f64)
        .filter(|number| (low..=high).contains(number))
        .map(|number| number as f32)
        .ok_or_else(|| invalid("scalar curve number is outside the supported range"))
}

fn handle(value: Option<&Value>, low: f64, high: f64, y_limit: f64) -> Result<[f32; 2]> {
    let Some(value) = value else {
        return Ok([0.0; 2]);
    };
    let handle = fields(Some(value), &["enabled", "magic", "x", "y"])?;
    flag(handle.get("magic"))?;
    let enabled = handle
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid("curve handle requires a boolean enabled flag"))?;
    let x = bounded_number(handle.get("x"), low, high)?;
    let y = bounded_number(handle.get("y"), -y_limit, y_limit)?;
    Ok(if enabled { [x, y] } else { [0.0; 2] })
}

fn relative_base(base: &Value) -> Result<[f32; 3]> {
    ensure!(
        base.as_str()
            .is_some_and(|base| base.len() <= MAX_TEXT_BYTES
                && base
                    .bytes()
                    .all(|byte| byte == b' ' || byte.is_ascii_graphic())
                && !base.starts_with(' ')
                && !base.contains(',')),
        invalid("relative timeline requires a space-separated vector string")
    );
    let base: [f32; 3] = super::numbers(base)
        .map_err(|_| invalid("invalid relative timeline vector base"))?
        .try_into()
        .map_err(|_| invalid("relative timeline requires three base components"))?;
    ensure!(
        base.iter()
            .all(|component| component.abs() <= MAX_VECTOR_COMPONENT),
        invalid("relative timeline base exceeds the supported range")
    );
    Ok(base)
}

fn link_key(value: &Value) -> Result<String> {
    fields(Some(value), &["key"])?
        .get("key")
        .and_then(Value::as_str)
        .filter(|key| {
            !key.is_empty() && key.len() <= MAX_TEXT_BYTES && !key.chars().any(char::is_control)
        })
        .map(str::to_owned)
        .ok_or_else(|| invalid("invalid timeline link key"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse_alpha(animation: &Value) -> Result<PropertyTimeline> {
        PropertyTimeline::parse(animation, "alpha", &Value::Null)
    }

    fn alpha(timeline: &PropertyTimeline, time: f64) -> f32 {
        let time = if timeline.clock.start_paused {
            0.0
        } else {
            time
        };
        let Sample::Scalar(value) = timeline.sample(timeline.clock.position(time)) else {
            panic!("Expected scalar sample");
        };
        value
    }

    fn simple() -> Value {
        json!({
            "c0": [{"frame": 0, "value": 0.25}, {"frame": 1, "value": 0.75}],
            "options": {"fps": 4, "length": 4, "mode": "single"}
        })
    }

    #[test]
    fn single_playback_blends_integer_samples_and_holds_both_edges() {
        let timeline = parse_alpha(&simple()).unwrap();
        assert_eq!(alpha(&timeline, 0.0), 0.25);
        assert_eq!(alpha(&timeline, 0.125), 0.5);
        assert_eq!(alpha(&timeline, 0.25), 0.75);
        assert_eq!(alpha(&timeline, 512.0), 0.75);
        assert!(!timeline.clock.finished(0.875));
        assert!(timeline.clock.finished(1.0));

        let mut paused = simple();
        paused["options"]["startpaused"] = json!(true);
        let timeline = parse_alpha(&paused).unwrap();
        assert_eq!(alpha(&timeline, 0.0), 0.25);
        assert_eq!(alpha(&timeline, 512.0), 0.25);
    }

    #[test]
    fn authored_bezier_shape_uses_both_tangents_and_clamps_to_edge_keys() {
        let timeline = parse_alpha(&json!({
            "options": {"fps": 8, "length": 32, "mode": "single", "wraploop": null},
            "c0": [
                {"frame": 4, "value": 0.125, "lockangle": true, "locklength": true,
                    "back": {"enabled": true, "magic": true, "x": -0.25, "y": 0},
                    "front": {"enabled": true, "magic": true, "x": 1, "y": 0.25}},
                {"frame": 12, "value": 0.75,
                    "back": {"enabled": true, "x": -0.375, "y": -0.125},
                    "front": {"enabled": true, "x": 0.5, "y": 0.125}},
                {"frame": 20, "value": 0.75,
                    "back": {"enabled": true, "x": -0.5, "y": 0.125},
                    "front": {"enabled": true, "x": 0.625, "y": -0.125}},
                {"frame": 28, "value": 0.25,
                    "back": {"enabled": true, "x": -0.75, "y": 0.1875},
                    "front": {"enabled": true, "x": 0.375, "y": 0}}
            ]
        }))
        .unwrap();
        assert_eq!(alpha(&timeline, 0.0), 0.125);
        // Solving the X cubics independently gives Y(8) = .40808 and
        // Y(9) = .48959. Allow the native inversion's finite frame tolerance.
        assert!((alpha(&timeline, 1.0625) - 0.44884).abs() < 0.001);
        assert!((alpha(&timeline, 2.0) - 0.84375).abs() < 0.001);
        assert!((alpha(&timeline, 3.0) - 0.51114).abs() < 0.001);
        assert_eq!(alpha(&timeline, 15.0), 0.25);
    }

    #[test]
    fn constant_full_alpha_never_exceeds_the_renderer_range_due_to_rounding() {
        let timeline = parse_alpha(&json!({
            "options": {"fps": 24, "length": 97, "mode": "single"},
            "c0": [{"frame": 0, "value": 1}, {"frame": 63, "value": 1}]
        }))
        .unwrap();
        for tick in 0..1000 {
            assert!((0.999999..=1.0).contains(&alpha(&timeline, f64::from(tick) / 120.0)));
        }
    }

    #[test]
    fn step_holds_integer_samples_until_the_incoming_key() {
        let mut input = simple();
        input["options"]["length"] = json!(8);
        input["c0"][1]["frame"] = json!(4);
        input["c0"][1]["step"] = json!(true);
        let timeline = parse_alpha(&input).unwrap();
        assert_eq!(alpha(&timeline, 0.75), 0.25);
        assert_eq!(alpha(&timeline, 0.875), 0.5);
        assert_eq!(alpha(&timeline, 1.0), 0.75);
    }

    fn relative() -> Value {
        json!({
            "relative": true,
            "c0": [{"frame":0,"value":1.0}, {"frame":1,"value":2.0}],
            "c1": [{"frame":0,"value":-1.5}, {"frame":1,"value":0.5}],
            "c2": [{"frame":0,"value":0.0}, {"frame":3,"value":0.0}],
            "options": {"fps":4,"length":4,"mode":"single","name":"Synthetic curve"}
        })
    }

    #[test]
    fn relative_vector_keys_add_each_authored_base_once_and_keep_depth_exact() {
        let track = PropertyTimeline::parse(&relative(), "origin", &json!("2 3 4.1")).unwrap();
        for (time, result) in [
            (0.0, [3.0, 1.5, 4.1]),
            (0.125, [3.5, 2.5, 4.1]),
            (0.25, [4.0, 3.5, 4.1]),
            (2.0, [4.0, 3.5, 4.1]),
            (0.125, [3.5, 2.5, 4.1]),
        ] {
            assert_eq!(
                track.sample(track.clock.position(time)),
                Sample::Vector(result)
            );
        }
    }

    #[test]
    fn linked_channel_uses_its_owners_integer_frames_and_fractional_phase() {
        let mut owner = relative();
        owner["options"]["children"] = json!([{"key":"alpha"}]);
        let owner = PropertyTimeline::parse(&owner, "scale", &json!("1 2 3")).unwrap();
        assert_eq!(owner.children, ["alpha"]);
        assert!(owner.parent.is_none());

        let mut child = simple();
        child["options"]["parent"] = json!({"key":"scale"});
        child["options"]["fps"] = json!(8);
        let child = parse_alpha(&child).unwrap();
        assert_eq!(child.parent.as_deref(), Some("scale"));
        assert!(child.children.is_empty());
        assert_eq!(
            child.sample(owner.clock.position(0.125)),
            Sample::Scalar(0.5)
        );
        assert_eq!(
            child.sample(child.clock.position(0.125)),
            Sample::Scalar(0.75)
        );
    }

    #[test]
    fn rejects_invalid_vector_domains_link_metadata_and_aggregate_key_count() {
        let valid_base = json!("2 3 4");
        for base in [
            json!([2, 3, 4]),
            json!("1 2"),
            json!("1 2 NaN"),
            json!("1,2,3"),
            json!("1 2 3e38"),
        ] {
            assert!(PropertyTimeline::parse(&relative(), "scale", &base).is_err());
        }
        let mut input = relative();
        input["c2"][0]["value"] = json!(1);
        assert!(PropertyTimeline::parse(&input, "origin", &valid_base).is_err());
        assert!(PropertyTimeline::parse(&input, "scale", &valid_base).is_ok());
        let mut input = relative();
        input["relative"] = json!(false);
        assert!(PropertyTimeline::parse(&input, "scale", &valid_base).is_err());
        let mut input = relative();
        input["options"]["length"] = json!(100);
        let channel = Value::Array(
            (0..43)
                .map(|frame| json!({"frame":frame,"value":0}))
                .collect(),
        );
        for key in ["c0", "c1", "c2"] {
            input[key] = channel.clone();
        }
        assert!(PropertyTimeline::parse(&input, "scale", &valid_base).is_err());
        for (field, value) in [
            ("parent", json!({"key":"origin","extra":true})),
            ("parent", json!({"key":""})),
            ("children", json!([{"key":0}])),
            ("name", json!("x".repeat(MAX_TEXT_BYTES + 1))),
        ] {
            let mut input = simple();
            input["options"][field] = value;
            assert!(parse_alpha(&input).is_err(), "{field}");
        }
    }

    #[test]
    fn rejects_unmodeled_shapes_and_fails_closed_on_excess_keys_or_alpha() {
        for (field, value) in [
            ("mode", json!("mirror")),
            ("random", json!(true)),
            ("wraploop", json!(true)),
            ("startpaused", json!("true")),
            ("length", json!(4097)),
            ("fps", json!(0)),
            ("events", json!([])),
        ] {
            let mut input = simple();
            input["options"][field] = value;
            assert!(parse_alpha(&input).is_err(), "{field}");
        }
        for keys in [
            json!([]),
            json!([{"frame": 1, "value": -0.1}]),
            json!([{"frame": 4, "value": 0.0}]),
            json!([{"frame": 1, "value": 0.0}, {"frame": 1, "value": 0.3}]),
            json!([{"frame": 1, "value": 1.0}, {"frame": 0, "value": 0.0}]),
            json!([{"frame": 0, "value": 0.0, "front": {"enabled": true, "x": 1.1, "y": 0.0}}]),
            json!([{"frame": 0, "value": 1.0, "front": {"enabled": true, "x": 0.5, "y": 0.1}}, {"frame": 1, "value": 1.0}]),
            Value::Array(vec![json!({"frame": 0, "value": 1.0}); MAX_KEYS + 1]),
        ] {
            let mut input = simple();
            input["c0"] = keys;
            assert!(parse_alpha(&input).is_err());
        }
        let mut input = simple();
        input["c1"] = json!([]);
        assert!(parse_alpha(&input).is_err());
    }
}
