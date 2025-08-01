# ROS pixi backend
The is a pixi build backend for ROS packages.

# Interesting links used in development
- RoboStack stacks:
  - https://github.com/RoboStack/ros-noetic
  - https://github.com/RoboStack/ros-humble
  - https://github.com/RoboStack/ros-jazzy
- RoboStack Vinca: https://github.com/RoboStack/vinca
- How `rosdep` works: https://docs.ros.org/en/humble/Tutorials/Intermediate/Rosdep.html#how-does-rosdep-work

# Questions
- How to handle the [distribution yaml files](https://github.com/RoboStack/ros-humble/blob/main/robostack.yaml)?
  - Should we fetch them from the specific robostack repo?
  - How should users add to this on the go?
  - Should there be logic to handle the full mapping to `conda-forge`?
- How to deal with `conditions` in a `depend`? e.g.: `<exec_depend condition="$ROS_VERSION == 1">catkin</exec_depend>`, `<depend condition="$PLATFORM == X3">hobot-multimedia-dev</depend>`
- How do we handle `target` specific dependencies in a `package.xml`?