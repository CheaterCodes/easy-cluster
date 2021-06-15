#![feature(map_first_last)]
use std::{collections::{BTreeSet, BinaryHeap, HashMap, HashSet}, error::Error, fs::File, io::BufWriter, path::Path};

use petgraph::{Graph, visit::IntoNodeReferences};
use png::Encoder;

use minecraft_schematics::{BlockState, Region, Schematic};

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
    let offset: (i32, i32) = (-20, 20);
    let width: i32 = 50;
    let cluster_size: u64 = 810;
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

        // Get lowest hash and see if it's sufficient
        let min_chunk = potential_chunks.peek();
        if min_chunk.is_some() && min_chunk.unwrap().hash <= max_hash {
            let chunk = potential_chunks.pop().unwrap();
            cluster_chunks.insert(chunk);
        }
        else {
            // Else add another row of chunks
            for z in offset.1 .. offset.1 + width {
                let long = ((z as u64) << 32) | ((((length + offset.0) as u64) << 32) >> 32);
                let hash = mix(long) & mask;
                if hash >= min_hash {
                    let chunk = Chunk {
                        x: length + offset.0,
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
    let chest = BlockState::new("minecraft:chest");
    let concrete = BlockState::new("minecraft:concrete");
    let mut region = Region::new("chests");

    let start = graph[graph.node_indices().min_by_key(|&i| dist(&(graph[i].x, graph[i].z), &offset)).unwrap()];
    let mut chunks_to_explore = HashSet::new();
    let mut chunks_connected = HashSet::new();
    chunks_to_explore.insert((start.x, start.z));
    chunks_connected.insert((start.x, start.z));

    while !chunks_to_explore.is_empty() {
        let current_chunks: Vec<(i32, i32)> = chunks_to_explore.drain().collect();
        for chunk in &current_chunks {
            let pos = (chunk.0 + 1, chunk.1);
            if chunks.contains_key(&pos) &&  !chunks_connected.contains(&pos) {
                let start = (chunk.0 * 16 + 8, 0, chunk.1 * 16 + 8);
                let end = (pos.0 * 16 + 8, 0, pos.1 * 16 + 8);
                region.fill(start.into(), end.into(), &concrete);
                region.set_block_state((chunk.0 * 16 + 15, 1, chunk.1 * 16 + 8).into(), &chest);
                chunks_connected.insert(pos);
                chunks_to_explore.insert(pos);
            }
            let pos = (chunk.0 - 1, chunk.1);
            if chunks.contains_key(&pos) &&  !chunks_connected.contains(&pos) {
                let start = (chunk.0 * 16 + 8, 0, chunk.1 * 16 + 8);
                let end = (pos.0 * 16 + 8, 0, pos.1 * 16 + 8);
                region.fill(start.into(), end.into(), &concrete);
                region.set_block_state((chunk.0 * 16 + 0, 1, chunk.1 * 16 + 8).into(), &chest);
                chunks_connected.insert(pos);
                chunks_to_explore.insert(pos);
            }
            let pos = (chunk.0, chunk.1 + 1);
            if chunks.contains_key(&pos) &&  !chunks_connected.contains(&pos) {
                let start = (chunk.0 * 16 + 8, 0, chunk.1 * 16 + 8);
                let end = (pos.0 * 16 + 8, 0, pos.1 * 16 + 8);
                region.fill(start.into(), end.into(), &concrete);
                region.set_block_state((chunk.0 * 16 + 8, 1, chunk.1 * 16 + 15).into(), &chest);
                chunks_connected.insert(pos);
                chunks_to_explore.insert(pos);
            }
            let pos = (chunk.0, chunk.1 - 1);
            if chunks.contains_key(&pos) &&  !chunks_connected.contains(&pos) {
                let start = (chunk.0 * 16 + 8, 0, chunk.1 * 16 + 8);
                let end = (pos.0 * 16 + 8, 0, pos.1 * 16 + 8);
                region.fill(start.into(), end.into(), &concrete);
                region.set_block_state((chunk.0 * 16 + 8, 1, chunk.1 * 16 + 0).into(), &chest);
                chunks_connected.insert(pos);
                chunks_to_explore.insert(pos);
            }
        }
    }

    let path = Path::new("out/chunks.litematic");
    let file = File::create(path)?;
    let mut buffer = BufWriter::new(file);
    let mut schematic = Schematic::new();
    schematic.set_name("ChunkGrid");
    schematic.add_region(region);
    schematic.write_to(&mut buffer)?;

    println!("Done!");

    Ok(())
}
