//! WO-5 T2 verification tool — emits one real, encoded MRD1 packet to a
//! file so the browser-side decoder (`client-apps/web/src/lib/ace/
//! mirandaTransport.ts`) can be tested against actual Rust-produced bytes
//! rather than against its own assumptions about the wire format.
//!
//! Run: `cargo run -p miranda-transport --example emit_test_packet -- <path>`

use miranda_core::{arkit, kinematic_joints, BlendshapeFrame, KinematicTransformFrame, Quaternion};
use miranda_transport::frame::encode_to_bytes;

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/mrd1_test_packet.bin".to_string());

    let mut weights = [0.0f32; 52];
    // Distinct, checkable values on a handful of channels so a JS decoder
    // bug (wrong offset, wrong endianness, wrong channel-name mapping)
    // shows up as a specific wrong number at a specific name, not just
    // "something is nonzero somewhere."
    weights[arkit::JAW_OPEN] = 0.625;
    weights[arkit::MOUTH_SMILE_LEFT] = 0.125;
    weights[arkit::MOUTH_SMILE_RIGHT] = 0.1875;
    weights[arkit::EYE_BLINK_LEFT] = 1.0;
    weights[arkit::TONGUE_OUT] = 0.0; // explicitly zero — never driven by the solver

    let blend = BlendshapeFrame {
        timestamp_us: 123_456_789,
        weights,
    };

    let mut joints = [Quaternion::IDENTITY; 4];
    joints[kinematic_joints::HEAD] = Quaternion::from_angle_x(0.3f32.to_radians());
    joints[kinematic_joints::SHOULDER_LEFT] = Quaternion::from_angle_z(2.0f32.to_radians());

    let kin = KinematicTransformFrame {
        timestamp_us: 123_456_789,
        joints,
        head_pitch_deg: 0.3,
        clavicle_rise: 0.4,
        _reserved: [0; 8],
    };

    let packet = encode_to_bytes(&blend, &kin);
    std::fs::write(&path, &packet).expect("failed to write packet");

    println!("wrote {} bytes to {path}", packet.len());
    println!("expected values for JS decoder cross-check:");
    println!("  timestampUs = 123456789");
    println!("  weights.jawOpen = 0.625");
    println!("  weights.mouthSmileLeft = 0.125");
    println!("  weights.mouthSmileRight = 0.1875");
    println!("  weights.eyeBlinkLeft = 1.0");
    println!("  weights.tongueOut = 0.0");
    println!("  kinematicTimestampUs = 123456789");
    println!("  headPitchDeg = 0.3");
    println!("  clavicleRise = 0.4");
    println!(
        "  joints.head ~ {{x: {:.6}, y: 0, z: 0, w: {:.6}}}",
        joints[kinematic_joints::HEAD].x,
        joints[kinematic_joints::HEAD].w
    );
    println!(
        "  joints.shoulderLeft ~ {{x: 0, y: 0, z: {:.6}, w: {:.6}}}",
        joints[kinematic_joints::SHOULDER_LEFT].z,
        joints[kinematic_joints::SHOULDER_LEFT].w
    );
}
