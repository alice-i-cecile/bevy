use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(MinimalPlugins)
        .add_startup_system(spawn_deck_system)
        .init_resource::<Total>()
        .init_resource::<Chips>()
        .add_system(report_state_system)
        .add_system(hit_me_system)
        .init_resource::<NextCard>()
        .add_system(next_card_system)
        .add_system(compute_total_system)
        .insert_resource(ResetDeck { reset_needed: true })
        .add_system(reset_deck_system)
        .run();
}

#[derive(Default)]
struct Total {
    value: u8,
}

#[derive(Default)]
struct Total {
    value: u8,
}

#[derive(Component)]
struct Card;

#[derive(Component)]
enum Suit {
    Spades,
    Clubs,
    Hearts,
    Diamonds,
}

#[derive(Component)]
enum Rank {
    Ace,
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Ten,
    Jack,
    Queen,
    King,
}

impl Rank {
    fn value(&self) -> u8 {
        match *self {
            Ace => 1,
            Two => 2,
            Three => 3,
            Four => 4,
            Five => 5,
            Six => 6,
            Seven => 7,
            Eight => 8,
            Nine => 9,
            Ten => 10,
            Jack => 10,
            Queen => 10,
            King => 10,
        }
    }
}

fn spawn_deck_system(mut commands: Commands) {
    for suit in Suit::iter() {
        for value in Rank::iter() {
            commands.spawn().insert(Card).insert(suit).insert(value);
        }
    }
}

fn report_state_system(query: Query<(&Suit, &Rank), With<InPlay>>, total: Res<Total>) {
    println!("The cards in play are:");

    for (suit, rank) in query.iter() {
        println!("The {} of {}", suit, rank);
    }

    println!("The current total is: {}", total.value);
}

fn hit_me_system() {
    println!("Would you like to take another card?")
}

#[derive(Default)]
struct NextCard(bool);
struct InPlay;
struct Played;

fn next_card_system(
    mut next_card: ResMut<NextCard>,
    mut query: Query<(Entity, Suit, Rank), Without<Played>>,
    mut commands: Commands,
    mut reset_deck: ResMut<ResetDeck>,
) {
    query.iter_mut().shuffle();

    let (card_entity, suit, rank) = query.iter().next();

    commands.entity(card_entity).insert(InPlay).insert(Played);

    if query.iter().len() == 0 {
        reset_deck.reset_needed = true;
    }
}

fn compute_total_system(query: Query<Rank, With<InPlay>>, mut total: ResMut<Total>) {
    total.value = 0;

    for rank in query.iter() {
        total.value += rank.value();
    }
}

struct ResetInPlay {
    reset_needed: bool,
}

fn check_total_system(total: Res<Total>, mut reset_in_play: ResMut<ResetInPlay>) {
    if total.value < 21 {
        reset_in_play.reset_needed = false;
    } else if total.value == 21 {
        reset_in_play.reset_needed = true;
    } else {
        reset_in_play.reset_needed = true;
    }
}

fn reset_in_play_system(
    mut reset: ResMut<ResetInPlay>,
    query: Query<Entity, With<Card>>,
    mut commands: Commands,
) {
    if reset.reset_needed {
        for card_entity in query.iter() {
            commands.entity(card_entity).remove::<InPlay>();
        }
        reset.reset_needed = false;
    }
}

struct ResetDeck {
    reset_needed: bool,
}

fn reset_deck_system(
    mut reset: ResMut<ResetDeck>,
    query: Query<Entity, (With<Card>, Without<InPlay>)>,
    mut commands: Commands,
) {
    if reset.reset_needed {
        for card_entity in query.iter() {
            commands.entity(card_entity).remove::<Played>();
        }
        reset.reset_needed = false;
    }
}
