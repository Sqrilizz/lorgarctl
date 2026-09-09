# MSA10W visual model

`msa10w/msa10w.obj` and its adjacent `msa10w.mtl` are a procedural, approximate
visual reconstruction of the black MSA10W pictured in this repository's product
photograph. Import the OBJ into Blender with the MTL beside it. The shell,
buttons, wheel, thumb buttons and simplified palm mark are modeled geometry.

This is not manufacturer CAD, a measured scan, a dimensionally accurate part,
or a printable replacement shell. The underside is intentionally simplified;
unseen switches, sensor placement and internal components are not invented.
The indicator color is a visual draft preview, not a claim about RGB placement
or the device's current state.

The app constructs the same mesh locally from `mouse_model.rs`, with no network
fetches or additional runtime assets. To export a fresh copy to a new directory:

```sh
target/release/lorgar-app --export-model /tmp/my-msa10w-model
```

The target directory must not exist. Export does not start GTK, the daemon, or
HID access. The source parameters are intentionally independent of the protocol.
