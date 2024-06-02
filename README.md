# Data World
Store data in the form of entities to provide all functionallity of bevy ECS.

Data will be stored in separate data worlds, split into static (read-only) and dynamic (mutable) data
which can be accessed using a Resource from the main world.
Trying to access data from the static world as mutable will first clone it into the dynamic world.
This allowes the static data to act like templates that can create variants in the form of dynamic data.

All data supports serialization backed by bevys reflection system.
This allows dynamic data to act like save files and static data like a database that can be setup either on first start or pre-build and shipped as a RON file.