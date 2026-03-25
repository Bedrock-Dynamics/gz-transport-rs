use std::io::Result;

fn main() -> Result<()> {
    let proto_files = [
        // Core primitives
        "proto/gz/msgs/time.proto",
        "proto/gz/msgs/header.proto",
        "proto/gz/msgs/vector2d.proto",
        "proto/gz/msgs/vector3d.proto",
        "proto/gz/msgs/quaternion.proto",
        "proto/gz/msgs/pose.proto",
        "proto/gz/msgs/pose_v.proto",
        "proto/gz/msgs/color.proto",
        "proto/gz/msgs/boolean.proto",
        "proto/gz/msgs/empty.proto",
        "proto/gz/msgs/double.proto",
        "proto/gz/msgs/double_v.proto",
        "proto/gz/msgs/float.proto",
        "proto/gz/msgs/float_v.proto",
        "proto/gz/msgs/uint32.proto",
        "proto/gz/msgs/lens.proto",
        // Geometry primitives
        "proto/gz/msgs/boxgeom.proto",
        "proto/gz/msgs/spheregeom.proto",
        "proto/gz/msgs/cylindergeom.proto",
        "proto/gz/msgs/conegeom.proto",
        "proto/gz/msgs/capsulegeom.proto",
        "proto/gz/msgs/ellipsoidgeom.proto",
        "proto/gz/msgs/planegeom.proto",
        "proto/gz/msgs/polylinegeom.proto",
        "proto/gz/msgs/meshgeom.proto",
        "proto/gz/msgs/heightmapgeom.proto",
        "proto/gz/msgs/imagegeom.proto",
        "proto/gz/msgs/geometry.proto",
        // Physics
        "proto/gz/msgs/axis.proto",
        "proto/gz/msgs/axis_aligned_box.proto",
        "proto/gz/msgs/inertial.proto",
        "proto/gz/msgs/density.proto",
        "proto/gz/msgs/friction.proto",
        "proto/gz/msgs/surface.proto",
        "proto/gz/msgs/wrench.proto",
        // Materials
        "proto/gz/msgs/material.proto",
        // Sensor types (base definitions)
        "proto/gz/msgs/sensor_noise.proto",
        "proto/gz/msgs/distortion.proto",
        "proto/gz/msgs/camera_lens.proto",
        "proto/gz/msgs/altimeter_sensor.proto",
        "proto/gz/msgs/air_speed_sensor.proto",
        "proto/gz/msgs/air_pressure_sensor.proto",
        "proto/gz/msgs/camerasensor.proto",
        "proto/gz/msgs/contactsensor.proto",
        "proto/gz/msgs/gps_sensor.proto",
        "proto/gz/msgs/imu_sensor.proto",
        "proto/gz/msgs/lidar_sensor.proto",
        "proto/gz/msgs/logical_camera_sensor.proto",
        "proto/gz/msgs/magnetometer_sensor.proto",
        "proto/gz/msgs/sensor.proto",
        // Sensor data messages
        "proto/gz/msgs/image.proto",
        "proto/gz/msgs/imu.proto",
        "proto/gz/msgs/navsat.proto",
        "proto/gz/msgs/magnetometer.proto",
        "proto/gz/msgs/fluid_pressure.proto",
        "proto/gz/msgs/altimeter.proto",
        "proto/gz/msgs/laserscan.proto",
        "proto/gz/msgs/battery.proto",
        // Scene elements
        "proto/gz/msgs/plugin.proto",
        "proto/gz/msgs/projector.proto",
        "proto/gz/msgs/particle_emitter.proto",
        "proto/gz/msgs/collision.proto",
        "proto/gz/msgs/visual.proto",
        "proto/gz/msgs/link.proto",
        "proto/gz/msgs/joint.proto",
        "proto/gz/msgs/model.proto",
        "proto/gz/msgs/light.proto",
        "proto/gz/msgs/fog.proto",
        "proto/gz/msgs/sky.proto",
        "proto/gz/msgs/scene.proto",
        // World control
        "proto/gz/msgs/spherical_coordinates.proto",
        "proto/gz/msgs/world_reset.proto",
        "proto/gz/msgs/world_control.proto",
        "proto/gz/msgs/physics.proto",
        "proto/gz/msgs/log_playback_stats.proto",
        "proto/gz/msgs/world_stats.proto",
        "proto/gz/msgs/clock.proto",
        "proto/gz/msgs/entity.proto",
        "proto/gz/msgs/entity_factory.proto",
        // Discovery (custom, not from upstream)
        "proto/gz/msgs/discovery.proto",
        "proto/gz/msgs/stringmsg.proto",
        "proto/gz/msgs/sdf_generator_config.proto",
        // Mobile robot
        "proto/gz/msgs/twist.proto",
    ];

    prost_build::Config::new().compile_protos(&proto_files, &["proto/"])?;

    // Rerun if any proto file changes
    for proto in &proto_files {
        println!("cargo:rerun-if-changed={}", proto);
    }

    Ok(())
}
