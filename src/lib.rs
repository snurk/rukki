use std::fs;
use std::fs::File;
use std::error::Error;
use std::io::Write;
use log::info;
use std::collections::HashSet;

mod graph;
//ASK why tests don't compile without the pub
pub mod graph_algos;
mod trio;
mod trio_walk;
mod pseudo_hap;

pub use graph::Graph;
pub use graph::Vertex;
pub use graph::Path;
pub use graph::Link;
pub use graph::Direction;

fn read_graph(graph_fn: &str) -> Result<Graph, Box<dyn Error>>  {
    info!("Reading graph from {:?}", graph_fn);
    let g = Graph::read(&fs::read_to_string(graph_fn)?);

    info!("Graph read successfully");
    info!("Node count: {}", g.node_cnt());
    info!("Link count: {}", g.link_cnt());
    Ok(g)
}

pub fn run_trio_analysis(graph_fn: &str, trio_markers_fn: &str,
    init_node_annotation_fn: &Option<String>, haplo_paths_fn: &Option<String>,
    gaf_paths: bool, low_cnt_thr: usize, ratio_thr: f32) -> Result<(), Box<dyn Error>> {
    info!("Reading graph from {:?}", graph_fn);
    let g = Graph::read(&fs::read_to_string(graph_fn)?);

    info!("Graph read successfully");
    info!("Node count: {}", g.node_cnt());
    info!("Link count: {}", g.link_cnt());

    //for n in g.all_nodes() {
    //    println!("Node: {} length: {} cov: {}", n.name, n.length, n.coverage);
    //}
    //for l in g.all_links() {
    //    println!("Link: {}", g.l_str(l));
    //}
    //write!(output, "{}", g.as_gfa())?;

    info!("Reading trio marker information from {}", trio_markers_fn);
    let trio_infos = trio::read_trio(&fs::read_to_string(trio_markers_fn)?);

    info!("Assigning initial parental groups to the nodes");
    let parental_groups = trio::assign_parental_groups(&g, &trio_infos, low_cnt_thr, ratio_thr);
    info!("Detecting homozygous nodes");
    //TODO parameterize
    let parental_groups = trio_walk::assign_homozygous(&g, parental_groups, 100_000);

    if let Some(output) = init_node_annotation_fn {
        info!("Writing initial node annotation to {}", output);
        let mut output = File::create(output)?;

        writeln!(output, "node\tlength\tmat:pat\tassignment\tcolor")?;
        for (node_id, n) in g.all_nodes().enumerate() {
            assert!(g.name2id(&n.name) == node_id);
            if let Some(assign) = parental_groups.get(node_id) {
                let color = match assign.group {
                    trio::TrioGroup::PATERNAL => "#8888FF",
                    trio::TrioGroup::MATERNAL => "#FF8888",
                    trio::TrioGroup::ISSUE => "#fbb117",
                    trio::TrioGroup::HOMOZYGOUS => "#c5d165",
                };
                writeln!(output, "{}\t{}\t{}\t{:?}\t{}", n.name, n.length, assign.info
                                                       , assign.group, color)?;
            }
        }
    }

    if let Some(output) = haplo_paths_fn {
        info!("Searching for haplo-paths, output in {}", output);
        let mut output = File::create(output)?;

        writeln!(output, "name\tpath\tassignment\tinit_node")?;
        let init_node_len_thr = 500_000;
        let mut path_searcher = trio_walk::HaploSearcher::new(&g,
            &parental_groups, init_node_len_thr);

        for (path, node_id, group) in path_searcher.find_all() {
            assert!(path.vertices().contains(&Vertex::forward(node_id)));
            //info!("Identified {:?} path: {}", group, path.print(&g));
            writeln!(output, "path_from_{}\t{}\t{:?}\t{}",
                g.node(node_id).name,
                path.print_format(&g, gaf_paths),
                group,
                g.node(node_id).name)?;
        }

        let used = path_searcher.used();

        for (node_id, n) in g.all_nodes().enumerate() {
            if !used.contains_key(&node_id) {
                let group_str = parental_groups.group(node_id)
                                    .map_or(String::from("NA"), |x| format!("{:?}", x));

                //println!("Unused node: {} length: {} group: {}", n.name, n.length, group_str);
                writeln!(output, "unused_{}_len_{}\t{}\t{}\t{}",
                    n.name,
                    n.length,
                    Path::new(Vertex::forward(node_id)).print_format(&g, gaf_paths),
                    group_str,
                    node_id)?;
            }
            //FIXME how many times should we report HOMOZYGOUS node?!
            //What if it has never been used? Are we confident enough?
        }

    }

    info!("All done");
    Ok(())
}

pub fn run_primary_alt_analysis(graph_fn: &str,
                                colors_fn: &Option<String>,
                                paths_fn: &Option<String>,
                                gaf_paths: bool) -> Result<(), Box<dyn Error>> {
    let g = read_graph(graph_fn)?;
    let unique_block_len = 500_000;
    let linear_blocks = pseudo_hap::pseudo_hap_decompose(&g, unique_block_len);

    if let Some(output) = colors_fn {
        info!("Writing node colors to {}", output);
        let mut output = File::create(output)?;

        let mut primary_nodes = HashSet::new();
        let mut alt_nodes = HashSet::new();
        let mut boundary_nodes = HashSet::new();

        for block in &linear_blocks {
            let p = block.instance_path();
            primary_nodes.extend(p.vertices()
                                 .iter().map(|&v| v.node_id));
            alt_nodes.extend(block.known_alt_nodes().iter().copied());
            boundary_nodes.extend([p.start().node_id, p.end().node_id]);
        }

        writeln!(output, "node\tlength\tassignment\tcolor")?;
        for (node_id, n) in g.all_nodes().enumerate() {
            assert!(g.name2id(&n.name) == node_id);
            let mut color = "#808080";
            let mut assign = "NA";
            if boundary_nodes.contains(&node_id) {
                assert!(!alt_nodes.contains(&node_id));
                color = "#fbb117";
                assign = "PRIMARY_BOUNDARY";
            } else if primary_nodes.contains(&node_id) {
                assert!(!alt_nodes.contains(&node_id));
                color = "#8888FF";
                assign = "PRIMARY";
            } else if alt_nodes.contains(&node_id) {
                color = "#FF8888";
                assign = "ALT";
            }
            writeln!(output, "{}\t{}\t{}\t{}", n.name, n.length, assign, color)?;
        }
    }

    let used : HashSet<usize> = linear_blocks.iter()
                                    .flat_map(|b| b.all_nodes())
                                    .collect();

    if let Some(output) = paths_fn {
        info!("Outputting paths in {}", output);
        let mut output = File::create(output)?;

        writeln!(output, "name\tlen\tpath\tassignment")?;

        for (block_id, block) in linear_blocks.into_iter().enumerate() {
            writeln!(output, "primary_{}\t{}\t{}\tPRIMARY",
                block_id,
                block.instance_path().total_length(&g),
                block.instance_path().print_format(&g, gaf_paths))?;
            for (alt_id, &known_alt) in block.known_alt_nodes().iter().enumerate() {
                writeln!(output, "alt_{}_{}\t{}\t{}\tALT",
                    block_id,
                    alt_id,
                    g.node(known_alt).length,
                    Path::new(Vertex::forward(known_alt)).print_format(&g, gaf_paths))?;
            }
        }

        for (node_id, n) in g.all_nodes().enumerate() {
            if !used.contains(&node_id) {
                writeln!(output, "unused_{}\t{}\t{}\tNA",
                    n.name,
                    n.length,
                    Path::new(Vertex::forward(node_id)).print_format(&g, gaf_paths))?;
            }
        }
    }

    info!("All done");
    Ok(())
}
