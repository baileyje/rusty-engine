# Rusty Engine (VROOM putputput VROOM)

## Overview
This library is intended to be an extensible base library to build simulations like games or sciency stuff. The goal of the engine will be 
to leverage multi-threaded architectures where possible. The engine will expose a pluggble component architecture for extending into higher 
level features (ECS, 3D render, etc).

### The Core
At its core the engine will run a primary logic loop on a dedicated thread, this loop will control the flow at a very fine-grain level. 
This loop will default to running as fast as it can while maintaining the time taken for each loop and making it available to any logic 
used in the simulation. Each pass of the loop will be refereed to as a frame. The loop will also maintain a fixed update cycle to provide 
reliable and possibly time-delta sensitive logic like physics.

After each update or fixed update frame, the engine will consume any simulation state data produced by the simulation loop and pass it to the
second dedicated render thread. The render thread loop will attempt to render a frame of data at whatever rate the render system supports. The render loop 
will wait on the logic loop if there are no frames available.


