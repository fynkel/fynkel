//! Native F3 registration/transaction slice. This uses ordinary value access;
//! efficient bulk views and registered scheduling are subsequent F3 work.
#[path = "registered_components/health.rs"]
mod health;
use fynkel_core::{Transform, World, WorldConfig};
use health::Health;

fn main() {
    let mut world = World::new(WorldConfig::default());
    world
        .register_component::<Health>(health::schema())
        .unwrap();
    let created = world
        .edit_gameplay(|edit| {
            let player = edit.spawn("Player", Transform::IDENTITY, None)?;
            edit.add_registered_defaults(player, "example.Health")?;
            Ok(player)
        })
        .unwrap();
    let player = created.resolve(created.value).unwrap();
    world
        .edit_gameplay(|edit| {
            let mut health = edit.registered::<Health>(player)?.expect("player health");
            health.hp -= 10.0;
            edit.set_registered(player, health)?;
            assert_eq!(edit.registered::<Health>(player)?.unwrap().hp, 90.0);
            Ok(())
        })
        .unwrap();
    assert_eq!(world.ecs().get::<Health>(player).unwrap().hp, 90.0);
    println!("Player Health = 90; registered outside core, committed to native ECS.");
}
