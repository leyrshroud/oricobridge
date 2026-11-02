pub const GS_V0: [u8; 4] = [0x1d, 0x76, 0x30, 0x00];

pub struct Page<'a> {
    pub commands: &'a [u8],
    pub row_bytes: usize,
    pub rows: Vec<&'a [u8]>,
}

pub struct Job<'a> {
    pub pages: Vec<Page<'a>>,
    pub trailer: &'a [u8],
}

impl<'a> Job<'a> {
    pub fn parse(data: &'a [u8]) -> Self {
        let mut pages: Vec<Page> = Vec::new();
        let mut pos = 0;

        while let Some(found) = find(&data[pos..], &GS_V0) {
            let start = pos + found;
            let Some(hdr) = data.get(start..start + 8) else {
                break;
            };

            let row_bytes = u16::from_le_bytes([hdr[4], hdr[5]]) as usize;
            let height = u16::from_le_bytes([hdr[6], hdr[7]]) as usize;
            if row_bytes == 0 {
                pos = start + 8;
                continue;
            }

            let image_start = start + 8;
            let image_end = data.len().min(image_start + row_bytes * height);
            let commands = &data[pos..start];
            let rows = data[image_start..image_end].chunks_exact(row_bytes);

            match pages.last_mut() {
                Some(page) if commands.is_empty() && page.row_bytes == row_bytes => {
                    page.rows.extend(rows)
                }
                _ => pages.push(Page {
                    commands,
                    row_bytes,
                    rows: rows.collect(),
                }),
            }
            pos = image_end;
        }

        Self {
            pages,
            trailer: &data[pos..],
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(row_bytes: usize, height: usize) -> Vec<u8> {
        let mut v = GS_V0.to_vec();
        v.extend((row_bytes as u16).to_le_bytes());
        v.extend((height as u16).to_le_bytes());
        v.resize(v.len() + row_bytes * height, 0xaa);
        v
    }

    #[test]
    fn splits_pages_on_driver_commands() {
        let mut data = vec![0x10, 0xff, 0xfe, 0x01];
        data.extend(image(204, 3));
        data.extend(image(204, 2));
        data.extend([0x10, 0x50, 0x10, 0xff, 0xfe, 0x45]);
        data.extend(image(204, 4));
        data.extend([0x10, 0x50]);

        let job = Job::parse(&data);

        assert_eq!(job.pages.len(), 2);
        assert_eq!(job.pages[0].rows.len(), 5);
        assert_eq!(job.pages[0].commands, [0x10, 0xff, 0xfe, 0x01]);
        assert_eq!(job.pages[1].rows.len(), 4);
        assert_eq!(job.pages[1].commands, [0x10, 0x50, 0x10, 0xff, 0xfe, 0x45]);
        assert_eq!(job.trailer, [0x10, 0x50]);
    }

    #[test]
    fn truncated_image() {
        let mut data = image(10, 5);
        data.truncate(8 + 10 * 3 + 4);
        assert_eq!(Job::parse(&data).pages[0].rows.len(), 3);
    }

    #[test]
    fn no_raster() {
        let job = Job::parse(b"hello");
        assert!(job.pages.is_empty());
        assert_eq!(job.trailer, b"hello");
    }
}
