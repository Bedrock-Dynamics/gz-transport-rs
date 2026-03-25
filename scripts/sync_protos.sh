#!/bin/bash
set -e

GZ_MSGS_TAG="gz-msgs10_10.3.0"
GZ_MSGS_REPO="https://raw.githubusercontent.com/gazebosim/gz-msgs"

PROTO_DIR="$(dirname "$0")/../proto/gz/msgs"

# All messages needed for scene graph and simulation
# Ordered by dependency (base types first)
PROTOS=(
    # Core primitives
    "header"
    "time"
    "vector2d"
    "vector3d"
    "quaternion"
    "pose"
    "pose_v"
    "color"
    "boolean"
    "empty"
    "double"
    "double_v"
    "float"
    "float_v"
    "uint32"
    "lens"

    # Geometry primitives
    "boxgeom"
    "spheregeom"
    "cylindergeom"
    "conegeom"
    "capsulegeom"
    "ellipsoidgeom"
    "planegeom"
    "polylinegeom"
    "meshgeom"
    "heightmapgeom"
    "imagegeom"
    "geometry"

    # Physics
    "axis"
    "axis_aligned_box"
    "inertial"
    "density"
    "friction"
    "surface"
    "wrench"

    # Materials
    "material"

    # Sensor types (base definitions)
    "altimeter_sensor"
    "air_speed_sensor"
    "air_pressure_sensor"
    "camerasensor"
    "contactsensor"
    "gps_sensor"
    "imu_sensor"
    "lidar_sensor"
    "logical_camera_sensor"
    "magnetometer_sensor"
    "sensor"
    "sensor_noise"
    "distortion"
    "camera_lens"

    # Sensor data messages
    "image"
    "imu"
    "navsat"
    "magnetometer"
    "fluid_pressure"
    "altimeter"
    "laserscan"
    "battery"

    # Scene elements
    "plugin"
    "projector"
    "particle_emitter"
    "collision"
    "visual"
    "link"
    "joint"
    "model"
    "light"
    "fog"
    "sky"
    "scene"

    # World control
    "spherical_coordinates"
    "world_reset"
    "world_control"
    "log_playback_stats"
    "world_stats"
    "clock"
    "entity"
    "entity_factory"

    # Discovery (custom, don't overwrite)
    # "discovery"
    # "stringmsg"
)

mkdir -p "$PROTO_DIR"

for proto in "${PROTOS[@]}"; do
    echo "Downloading ${proto}.proto..."
    curl -sfL "$GZ_MSGS_REPO/$GZ_MSGS_TAG/proto/gz/msgs/${proto}.proto" \
        -o "$PROTO_DIR/${proto}.proto"
done

echo "Synced ${#PROTOS[@]} protos from gz-msgs@$GZ_MSGS_TAG"
