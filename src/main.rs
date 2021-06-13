#![feature(map_first_last)]
use std::{collections::{BTreeSet, BinaryHeap, HashMap}, error::Error, fs::File, io::BufWriter, path::Path};

use nbt::{CompoundTag, encode::write_gzip_compound_tag};
use petgraph::{Graph, visit::IntoNodeReferences};
use png::Encoder;

enum ChunkType {
    Connecting,
    Target
}

fn mix(val: u64) -> u64 {
    let mut hashed = val.wrapping_mul(0x9E3779B97F4A7C15);
    hashed ^= hashed >> 32;
    hashed ^= hashed >> 16;
    hashed
}

fn dist(pos1: &(i32, i32), pos2: &(i32, i32)) -> i32 {
    (pos1.0 - pos2.0).abs() + (pos1.1 - pos2.1).abs()
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
struct Chunk {
    x: i32,
    z: i32,
    hash: u64
}

impl Ord for Chunk {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.hash.cmp(&self.hash)
            .then_with(|| self.x.cmp(&other.x))
            .then_with(|| self.z.cmp(&other.z))
    }
}

impl PartialOrd for Chunk {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let offset: (i32, i32) = (0, 0);
    let width: i32 = 50;
    let cluster_size: u64 = 800;
    let hash_size: u64 = 2048;

    assert!(hash_size.is_power_of_two());
    let mask = hash_size - 1;

    println!("Looking for {} cluster chunks...", cluster_size);

    let mut cluster_chunks = BTreeSet::new();
    let mut potential_chunks: BinaryHeap<Chunk> = BinaryHeap::new();

    let min_hash = hash_size - cluster_size;
    let mut length: i32 = 0;

    while cluster_chunks.len() < cluster_size as usize {
        let max_hash = min_hash + cluster_chunks.len() as u64;
        println!("Min: {}, Max: {}", min_hash, max_hash);

        // Get lowest hash and see if it's sufficient
        let min_chunk = potential_chunks.peek();
        println!("Min Chunk: {:?}", min_chunk);
        if min_chunk.is_some() && min_chunk.unwrap().hash <= max_hash {
            let chunk = potential_chunks.pop().unwrap();
            cluster_chunks.insert(chunk);
        }
        else {
            // Else add another row of chunks
            for z in offset.1 .. offset.1 + width {
                let long = ((z as u64) << 32) | (((length as u64) << 32) >> 32);
                let hash = mix(long) & mask;
                if hash >= min_hash {
                    let chunk = Chunk {
                        x: length,
                        z: z,
                        hash: hash
                    };
                    potential_chunks.push(chunk);
                }
            }
            length = length + 1;
        }
    }

    let size = (length, width);

    println!("Found {} valid cluster chunks!", cluster_chunks.len());
    println!("Searched area: {} x {} chunks", size.0, size.1);


    println!("Generating tree...");

    // Generate minimum spanning tree (Prim's method, not efficient)
    let mut graph = Graph::<Chunk, i32, _>::new_undirected();
    graph.add_node(cluster_chunks.pop_first().unwrap());

    while !cluster_chunks.is_empty() {
        let (index_a, chunk, d) = graph.node_references().map(|(index, node)| {
            let (&chunk, d) = cluster_chunks.iter()
                .map(|chunk| (chunk, dist(&(chunk.x, chunk.z), &(node.x, node.z))))
                .min_by_key(|(_chunk, d)| *d).unwrap();
            return (index, chunk, d);
        }).min_by_key(|(_index, _chunk, d)| *d).unwrap();

        let index_b = graph.add_node(chunk);
        graph.add_edge(index_a, index_b, d);

        cluster_chunks.remove(&chunk);
    }

    println!("Collecting chunks...");
    let mut chunks = HashMap::new();

    let mut img_data = vec![0; size.0 as usize * size.1 as usize];

    for edge in graph.edge_indices() {
        let (index_a, index_b) = graph.edge_endpoints(edge).unwrap();
        let pos_a = graph[index_a];
        let pos_b = graph[index_b];
        
        for x in pos_a.x.min(pos_b.x) ..= pos_a.x.max(pos_b.x) {
            chunks.insert((x, pos_a.z), ChunkType::Connecting);
        }
        
        for z in pos_a.z.min(pos_b.z) ..= pos_a.z.max(pos_b.z) {
            chunks.insert((pos_b.x, z), ChunkType::Connecting);
        }
    }

    for node in graph.node_indices() {
        let pos = graph[node];
        let edge = chunks.insert((pos.x, pos.z), ChunkType::Target);
        if edge.is_none() {
            println!("Missing edge at {:?}", (pos.x, pos.z));
        }
    }

    println!("Total chunks loaded: {}", chunks.len());

    println!("Generating image...");
    for (chunk, typ) in &chunks {
        let data = &mut img_data[((chunk.0 - offset.0) + (chunk.1 - offset.1) * size.0) as usize];
        match typ {
            ChunkType::Connecting => *data = 127,
            ChunkType::Target => *data = 255,
        }
    }

    let path = Path::new("out/chunks.png");
    let file = File::create(path)?;
    let buffer = BufWriter::new(file);
    let mut encoder = Encoder::new(buffer, size.0 as u32, size.1 as u32);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&img_data)?;


    println!("Generating schematic...");
    let mut schematic = CompoundTag::new();
    let mut regions = CompoundTag::new();
    let mut chests = CompoundTag::new();

    let mut region_pos = CompoundTag::new();
    region_pos.insert_i32("x", offset.0 * 16);
    region_pos.insert_i32("y", 0);
    region_pos.insert_i32("z", offset.1 * 16);
    chests.insert_compound_tag("Position", region_pos);

    let mut region_size = CompoundTag::new();
    region_size.insert_i32("x", size.0 as i32 * 16);
    region_size.insert_i32("y", 1);
    region_size.insert_i32("z", size.1 as i32 * 16);
    chests.insert_compound_tag("Size", region_size);

    let mut palette = Vec::new();
    let mut air = CompoundTag::new();
    air.insert_str("Name", "minecraft:air");
    palette.push(air);
    let mut chest = CompoundTag::new();
    chest.insert_str("Name", "minecraft:chest");
    palette.push(chest);

    let bits = 2;
    let mut block_states: Vec<i64> = vec![0; (size.0 * size.1 * 16 * 16 / 64 * bits) as usize];

    for (chunk, _typ) in &chunks {
        let pos_x = (chunk.0 + 1, chunk.1);
        if chunks.contains_key(&pos_x) {
            let pos = (15, 8);
            let index = ((chunk.1 - offset.1) * 16 + pos.1) * size.0 * 16 + (chunk.0 - offset.0) * 16 + pos.0;
            block_states[(index * bits / 64) as usize] |= 1 << (index * bits % 64);
        }
        let neg_x = (chunk.0 - 1, chunk.1);
        if chunks.contains_key(&neg_x) {
            let pos = (0, 7);
            let index = ((chunk.1 - offset.1) * 16 + pos.1) * size.0 * 16 + (chunk.0 - offset.0) * 16 + pos.0;
            block_states[(index * bits / 64) as usize] |= 1 << (index * bits % 64);
        }
        let pos_z = (chunk.0, chunk.1 + 1);
        if chunks.contains_key(&pos_z) {
            let pos = (8, 15);
            let index = ((chunk.1 - offset.1) * 16 + pos.1) * size.0 * 16 + (chunk.0 - offset.0) * 16 + pos.0;
            block_states[(index * bits / 64) as usize] |= 1 << (index * bits % 64);
        }
        let neg_z = (chunk.0, chunk.1 - 1);
        if chunks.contains_key(&neg_z) {
            let pos = (7, 0);
            let index = ((chunk.1 - offset.1) * 16 + pos.1) * size.0 * 16 + (chunk.0 - offset.0) * 16 + pos.0;
            block_states[(index * bits / 64) as usize] |= 1 << (index * bits % 64);
        }
    }
    chests.insert_i64_vec("BlockStates", block_states);
    chests.insert_compound_tag_vec("Entities", Vec::new());
    chests.insert_compound_tag_vec("TileEntities", Vec::new());
    chests.insert_compound_tag_vec("BlockStatePalette", palette);

    regions.insert("Chests", chests);
    schematic.insert_i32("Version", 4);
    schematic.insert_compound_tag("Regions", regions);
    schematic.insert_compound_tag("Metadata", CompoundTag::new());
    let path = Path::new("out/chunks.litematic");
    let file = File::create(path)?;
    let mut buffer = BufWriter::new(file);
    write_gzip_compound_tag(&mut buffer, &schematic)?;

    println!("Done!");

    Ok(())
}
