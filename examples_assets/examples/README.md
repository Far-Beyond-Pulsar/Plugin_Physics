# Physics examples

Read-only examples of the three file types the Physics plugin registers. Copy
one into your project's assets to start from it. All are plain JSON.

## Materials (`materials/*.physmat`)

Friction (0 is ice, about 1 is rubber on asphalt), restitution (the fraction
of speed kept after an impact), how each combines with the surface it touches
(`average`, `min`, `multiply`, `max`; when two materials ask for different
rules the higher in that order wins), density in kg/m3 (a body with mass 0
takes its mass from its collider volume times this) and a `surface` that
footsteps and impact effects react to. `description` says what each shows.

| File | Shows |
|---|---|
| `default` | the engine's defaults: what a new file or an unauthored collider uses |
| `concrete` | high friction, dead, heavy |
| `steel` | `min` friction combine, very dense |
| `wood` | a floating material (density under 1000) |
| `rubber` | `max` on both rules: grippy and bouncy whatever it touches |
| `ice` | near-zero friction with `min` |
| `glass` | a surface type other systems react to |
| `sand` | `multiply` restitution: kills any bounce |
| `grass` | soft ground |
| `bouncy_ball` | restitution 0.9 |
| `foam` | very low density |
| `flesh` | a creature body |

## Collision layers (`collision_layers/*.physlayers`)

A list of named layers; each lists the layers it collides with. A pair
collides only when **both** layers list each other (the editor flags a
one-sided entry, which silently does nothing). A layer's position is its bit
in the 32-bit collision group masks, so there are at most 32.

| File | Shows |
|---|---|
| `simple` | the smallest useful table: solid things and a sensor layer |
| `platformer` | player, enemies, projectiles, pickups and triggers with who-hits-whom rules |
| `vehicles` | wheels that only touch the road, debris that ignores other debris |

## Skeletal physics assets (`skeletal_physics/*.physasset`)

The rigid bodies and joints of one skeletal mesh (a ragdoll, a hinged door).
`skeletal_mesh` is the id of the mesh the asset is for and each `bone_id` names
a bone of that mesh's skeleton. The ids here belong to no mesh, so these are
shapes to learn from: to use one, set `skeletal_mesh` to your mesh's id and
rename the bones, or let the editor build a fresh first pass from your
skeleton. Shapes are `box` (half-extents), `sphere` (radius in x), `capsule`
and `cylinder` (radius, half-height); motion is `dynamic`, `kinematic` or
`fixed`; joint limits are in degrees.

| File | Shows |
|---|---|
| `humanoid_ragdoll` | ball sockets and hinges, a kinematic pelvis |
| `quadruped_partial` | a fixed root, a partial rig |
| `door_hinge` | one hinged body and a fixed frame |
