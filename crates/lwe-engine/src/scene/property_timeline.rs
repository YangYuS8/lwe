use anyhow::{Result, ensure};
use serde_json::{Map, Value};

const MAX_KEYS: usize = 128;
const MAX_LENGTH: u64 = 4096;
const MAX_FPS: f64 = 240.0;

/// A bounded scalar opacity track in the Scene's accepted time domain.
#[derive(Debug, Clone)]
pub(super) struct ScalarTimeline {
    keys: Vec<Key>,
    length: u32,
    duration: f32,
    frame_seconds: f32,
    start_paused: bool,
}

#[derive(Debug, Clone, Copy)]
struct Key {
    frame: u32,
    value: f32,
    incoming: [f32; 2],
    outgoing: [f32; 2],
    step: bool,
}

impl ScalarTimeline {
    pub(super) fn parse(animation: &Value) -> Result<Self> {
        let animation = fields(Some(animation), &["c0", "options"])?;
        let options = fields(
            animation.get("options"),
            &["fps", "length", "mode", "random", "startpaused", "wraploop"],
        )?;
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
        let channel = animation
            .get("c0")
            .and_then(Value::as_array)
            .filter(|keys| (1..=MAX_KEYS).contains(&keys.len()))
            .ok_or_else(|| invalid("scalar keys are missing or exceed their budget"))?;
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
            let key = Key {
                frame,
                value: bounded_number(key.get("value"), 0.0, 1.0)?,
                incoming: handle(key.get("back"), -1.0, 0.0)?,
                outgoing: handle(key.get("front"), 0.0, 1.0)?,
                step: flag(key.get("step"))?,
            };
            if let Some(previous) = keys.last() {
                ensure!(
                    previous.frame < key.frame,
                    invalid("keys must be strictly ordered")
                );
                // Bound the whole Bezier alpha curve by its convex control hull.
                if !key.step {
                    ensure!(
                        (0.0..=1.0).contains(&(previous.value + previous.outgoing[1]))
                            && (0.0..=1.0).contains(&(key.value + key.incoming[1])),
                        invalid("alpha controls exceed the supported range")
                    );
                }
            }
            keys.push(key);
        }
        Ok(Self {
            keys,
            length,
            duration: length as f32 / fps,
            frame_seconds: 1.0 / fps,
            start_paused,
        })
    }

    pub(super) fn sample(&self, time: f64) -> f32 {
        // The native clock stops at the single-play duration, but the outer
        // sampler still publishes the held value on each update.
        let time = if self.start_paused {
            0.0
        } else {
            (time.max(0.0) as f32).min(self.duration)
        };
        // Native rebuilds the curve at integer frames, then blends adjacent
        // cached samples with the fractional phase in seconds.
        let frame = ((time / self.frame_seconds) as u32).min(self.length - 1);
        let phase = (time % self.frame_seconds) / self.frame_seconds;
        let value = self.at_frame(frame) * (1.0 - phase) + self.at_frame(frame + 1) * phase;
        value.clamp(0.0, 1.0)
    }

    fn at_frame(&self, frame: u32) -> f32 {
        let next = self.keys.partition_point(|key| key.frame <= frame);
        let [left, right] = match next {
            0 => return self.keys[0].value,
            next if next == self.keys.len() => return self.keys[next - 1].value,
            next => [self.keys[next - 1], self.keys[next]],
        };
        if frame == left.frame || right.step {
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

fn handle(value: Option<&Value>, low: f64, high: f64) -> Result<[f32; 2]> {
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
    let y = bounded_number(handle.get("y"), -1.0, 1.0)?;
    Ok(if enabled { [x, y] } else { [0.0; 2] })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn simple() -> Value {
        json!({
            "c0": [{"frame": 0, "value": 0.25}, {"frame": 1, "value": 0.75}],
            "options": {"fps": 4, "length": 4, "mode": "single"}
        })
    }

    #[test]
    fn single_playback_blends_integer_samples_and_holds_both_edges() {
        let timeline = ScalarTimeline::parse(&simple()).unwrap();
        assert_eq!(timeline.sample(0.0), 0.25);
        assert_eq!(timeline.sample(0.125), 0.5);
        assert_eq!(timeline.sample(0.25), 0.75);
        assert_eq!(timeline.sample(512.0), 0.75);

        let mut paused = simple();
        paused["options"]["startpaused"] = json!(true);
        let timeline = ScalarTimeline::parse(&paused).unwrap();
        assert_eq!(timeline.sample(0.0), 0.25);
        assert_eq!(timeline.sample(512.0), 0.25);
    }

    #[test]
    fn authored_bezier_shape_uses_both_tangents_and_clamps_to_edge_keys() {
        let timeline = ScalarTimeline::parse(&json!({
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
        assert_eq!(timeline.sample(0.0), 0.125);
        // Solving the X cubics independently gives Y(8) = .40808 and
        // Y(9) = .48959. Allow the native inversion's finite frame tolerance.
        assert!((timeline.sample(1.0625) - 0.44884).abs() < 0.001);
        assert!((timeline.sample(2.0) - 0.84375).abs() < 0.001);
        assert!((timeline.sample(3.0) - 0.51114).abs() < 0.001);
        assert_eq!(timeline.sample(15.0), 0.25);
    }

    #[test]
    fn constant_full_alpha_never_exceeds_the_renderer_range_due_to_rounding() {
        let timeline = ScalarTimeline::parse(&json!({
            "options": {"fps": 24, "length": 97, "mode": "single"},
            "c0": [{"frame": 0, "value": 1}, {"frame": 63, "value": 1}]
        }))
        .unwrap();
        for tick in 0..1000 {
            assert!((0.999999..=1.0).contains(&timeline.sample(f64::from(tick) / 120.0)));
        }
    }

    #[test]
    fn step_holds_integer_samples_until_the_incoming_key() {
        let mut input = simple();
        input["options"]["length"] = json!(8);
        input["c0"][1]["frame"] = json!(4);
        input["c0"][1]["step"] = json!(true);
        let timeline = ScalarTimeline::parse(&input).unwrap();
        assert_eq!(timeline.sample(0.75), 0.25);
        assert_eq!(timeline.sample(0.875), 0.5);
        assert_eq!(timeline.sample(1.0), 0.75);
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
            assert!(ScalarTimeline::parse(&input).is_err(), "{field}");
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
            assert!(ScalarTimeline::parse(&input).is_err());
        }
        let mut input = simple();
        input["c1"] = json!([]);
        assert!(ScalarTimeline::parse(&input).is_err());
    }
}
