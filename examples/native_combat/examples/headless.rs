use fynkel_core::*;
use fynkel_example_native_combat::{Combat, Health};
fn main() {
    let mut world = World::new(Default::default());
    world
        .install_extension(&Combat)
        .expect("compatible package");
    let created = world
        .edit_gameplay(|edit| {
            let player = edit.spawn("Player", Transform::IDENTITY, None)?;
            edit.add_registered_defaults(player, "example.combat.Health")?;
            Ok(player)
        })
        .unwrap();
    let player = created.resolve(created.value).unwrap();
    assert_eq!(world.step().rejected, 0);
    let health = world.ecs().get::<Health>(player).unwrap();
    assert!(health.hp < 100.);
    println!(
        "Player health after one registered native system tick: {}",
        health.hp
    );
}
