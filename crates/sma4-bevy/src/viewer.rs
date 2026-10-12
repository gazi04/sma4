//! Graphics browser over the extracted tile sheets (`gfx/*.png`). PageUp/PageDown switch.

use std::path::PathBuf;

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use sma4_assets::{Rgba, gfx_sheets};

use crate::AssetDir;

pub struct ViewerPlugin;

impl Plugin for ViewerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup)
            .add_systems(Update, (browse, redraw.run_if(resource_changed::<Viewer>)).chain());
    }
}

#[derive(Resource)]
struct Viewer {
    sheets: Vec<PathBuf>,
    index: usize,
}

#[derive(Component)]
struct Sheet;

#[derive(Component)]
struct Label;

fn setup(mut commands: Commands, dir: Res<AssetDir>) {
    let sheets = gfx_sheets(&dir.0);
    info!("viewer: {} graphics sheets", sheets.len());
    commands.insert_resource(Viewer { sheets, index: 0 });
    commands.spawn((Sheet, Sprite::default(), Transform::default()));
    commands.spawn((
        Label,
        Text::new(""),
        TextFont { font_size: FontSize::Px(16.0), ..default() },
        Node { position_type: PositionType::Absolute, top: px(4), left: px(4), ..default() },
    ));
}

fn browse(keys: Res<ButtonInput<KeyCode>>, mut viewer: ResMut<Viewer>) {
    let n = viewer.sheets.len();
    if n == 0 {
        return;
    }
    if keys.just_pressed(KeyCode::PageDown) {
        viewer.index = (viewer.index + 1) % n;
    }
    if keys.just_pressed(KeyCode::PageUp) {
        viewer.index = (viewer.index + n - 1) % n;
    }
}

fn redraw(
    viewer: Res<Viewer>,
    mut images: ResMut<Assets<Image>>,
    mut sheet: Single<&mut Sprite, With<Sheet>>,
    mut label: Single<&mut Text, With<Label>>,
) {
    let Some(path) = viewer.sheets.get(viewer.index) else {
        label.0 = "no graphics sheets: run sma4-extract first".into();
        return;
    };
    let img = match Rgba::read_png(path) {
        Ok(i) => i,
        Err(e) => {
            label.0 = e.to_string();
            return;
        }
    };
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
    label.0 = format!(
        "sheet {}/{}  ROM 0x{name}  {}x{} px  [PgUp/PgDn]",
        viewer.index + 1,
        viewer.sheets.len(),
        img.width,
        img.height
    );
    sheet.image = images.add(Image::new(
        Extent3d { width: img.width, height: img.height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        img.pixels,
        TextureFormat::Rgba8UnormSrgb,
        // Sprites need the image in the main world too (for their size).
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    ));
}
