use std::{collections::HashMap, io::{Error, Write}, ops::{Add, Sub}, usize};

use nbt::{CompoundTag, encode::write_gzip_compound_tag};

#[derive(PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub struct BlockPos {
    x: i32,
    y: i32,
    z: i32
}

impl BlockPos {
    pub fn new(x: i32, y: i32, z: i32)  -> BlockPos {
        BlockPos {x, y, z}
    }

    pub fn zero() -> BlockPos {
        BlockPos::new(0, 0, 0)
    }

    pub fn one() -> BlockPos {
        BlockPos::new(1, 1, 1)
    }

    pub fn min(self, other: BlockPos) -> BlockPos {
        BlockPos {
            x: self.x.min(other.x),
            y: self.y.min(other.y),
            z: self.z.min(other.z),
        }
    }

    pub fn max(self, other: BlockPos) -> BlockPos {
        BlockPos {
            x: self.x.max(other.x),
            y: self.y.max(other.y),
            z: self.z.max(other.z),
        }
    }

    pub fn to_tag(&self) -> CompoundTag {
        let mut tag = CompoundTag::new();
        tag.insert_i32("x", self.x);
        tag.insert_i32("y", self.y);
        tag.insert_i32("z", self.z);
        tag
    }
}

impl Default for BlockPos {
    fn default() -> Self {
        BlockPos::zero()
    }
}

impl Add for BlockPos {
    type Output = BlockPos;

    fn add(self, rhs: Self) -> Self::Output {
        BlockPos {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z
        }
    }
}

impl Sub for BlockPos {
    type Output = BlockPos;

    fn sub(self, rhs: Self) -> Self::Output {
        BlockPos {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z
        }
    }
}

impl From<(i32, i32, i32)> for BlockPos {
    fn from(val: (i32, i32, i32)) -> Self {
        BlockPos {
            x: val.0,
            y: val.1,
            z: val.2
        }
    }
}

#[derive(PartialEq, Eq, Hash, Clone)]
pub struct BlockState<'a> {
    name: &'a str
}

impl<'a> BlockState<'a> {
    pub const fn new(name: &str) -> BlockState {
        BlockState {
            name: name
        }
    }

    pub fn to_tag(&self) -> CompoundTag {
        let mut tag = CompoundTag::new();
        tag.insert_str("Name", self.name);
        tag
    }
}

pub struct Region<'a> {
    name: &'a str,
    blocks: HashMap<BlockPos, &'a BlockState<'a>>
}

impl<'a> Region<'a> {
    pub fn new(name: &str) -> Region {
        Region {
            name: name,
            blocks: HashMap::new()
        }
    }

    pub fn set_block_state(&mut self, pos: BlockPos, state: &'a BlockState) {
        self.blocks.insert(pos, state);
    }

    pub fn fill(&mut self, start: BlockPos, end: BlockPos, state: &'a BlockState) {
        let min = BlockPos::min(start, end);
        let max = BlockPos::max(start, end);

        for z in min.z ..= max.z {
            for y in min.y ..= max.y {
                for x in min.x ..= max.x {
                    self.set_block_state(BlockPos::new(x, y, z), state);
                }
            }
        }
    }

    pub fn to_tag(&self) -> CompoundTag {
        let position = self.blocks.keys().map(|p| *p).reduce(BlockPos::min).unwrap_or(BlockPos::zero());
        let blocks = self.blocks.iter().map(|(&pos, &state)| (pos - position, state)).collect::<HashMap<_, _>>();
        let size = blocks.keys().map(|p| *p).reduce(BlockPos::max).map(|pos| pos + BlockPos::one()).unwrap_or(BlockPos::zero());

        let mut palette = HashMap::new();
        let air = BlockState::new("minecraft:air");
        palette.insert(&air, 0);
        for (_, &state) in &blocks {
            if !palette.contains_key(state) {
                palette.insert(state, palette.len());
            }
        }
        let mut palette_tags = vec![None; palette.len()];
        for (&state, &index) in &palette {
            palette_tags[index] = Some(state.to_tag());
        }

        let palette_tags = palette_tags.into_iter().map(|t| t.unwrap()).collect::<Vec<_>>();
        let bits = 64 - (palette.len() as u64 - 1).leading_zeros();
        let bits: u32 = bits.min(2);

        let mut block_states: Vec<i64> = vec![0; size.x as usize * size.y as usize * size.z as usize * bits as usize / 64 as usize];

        for (&pos, &state) in &blocks {
            let state_index =
                pos.y as usize * size.z as usize * size.x as usize +
                pos.z as usize * size.x as usize + 
                pos.x as usize;
            let long_index = state_index * bits as usize / 64 as usize;
            let bit_index = state_index as u32 * bits % 64;
            let state_bits = *palette.get(state).unwrap() as i64;
            
            block_states[long_index] |= state_bits << bit_index;
            if bit_index + bits > 64 {
                block_states[long_index + 1] |= state_bits >> (64 - bit_index);
            }
        }

        let mut region_tag = CompoundTag::new();
        region_tag.insert_compound_tag("Position", position.to_tag());
        region_tag.insert_compound_tag("Size", size.to_tag());
        region_tag.insert_compound_tag_vec("BlockStatePalette", palette_tags);
        region_tag.insert_i64_vec("BlockStates", block_states);
        region_tag.insert_compound_tag_vec("Entities", Vec::new());
        region_tag.insert_compound_tag_vec("TileEntities", Vec::new());
        region_tag.insert_compound_tag_vec("PendingBlockTick", Vec::new());

        region_tag
    }
}

pub struct Schematic<'a> {
    regions: Vec<Region<'a>>,
    name: Option<&'a str>,
    author: Option<&'a str>,
    description: Option<&'a str>
}

impl<'a> Schematic<'a> {
    pub fn new() -> Schematic<'a> {
        Schematic {
            regions: Vec::new(),
            name: None,
            author: None,
            description: None
        }
    }

    pub fn add_region(&mut self, region: Region<'a>) {
        self.regions.push(region);
    }

    pub fn set_name(&mut self, name: &'a str) {
        self.name = Some(name);
    }

    pub fn set_author(&mut self, author: &'a str) {
        self.author = Some(author);
    }

    pub fn set_description(&mut self, description: &'a str) {
        self.description = Some(description);
    }

    pub fn to_tag(&self) -> CompoundTag {
        let mut metadata = CompoundTag::new();
        if let Some(name) = self.name {
            metadata.insert_str("Name", name);
        }
        if let Some(author) = self.author {
            metadata.insert_str("Name", author);
        }
        if let Some(description) = self.description {
            metadata.insert_str("Name", description);
        }
        metadata.insert_i32("RegionCount", self.regions.len() as i32);

        let mut regions = CompoundTag::new();
        for region in &self.regions {
            regions.insert_compound_tag(region.name, region.to_tag());
        }

        let mut schematic = CompoundTag::new();
        schematic.insert_compound_tag("Metadata", metadata);
        schematic.insert_compound_tag("Regions", regions);
        schematic.insert_i32("Version", 4);

        schematic
    }

    pub fn write_to<W: Write>(&self, writer: &mut W) -> Result<(), Error> {
        write_gzip_compound_tag(writer, &self.to_tag())
    }
}
