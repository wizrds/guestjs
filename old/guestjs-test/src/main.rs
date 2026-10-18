use guestjs::prelude::*;

const GUEST_MODULE: &str = r#"
import { Point, Shape } from "host:geometry";

export function run(): string {
    const p = new Point(3, 4);
    p.x = 6;

    const shape = new Shape([p, Point.origin()]);
    const first = shape.points[0];
    const nearest = shape.nearest(new Point(5, 5));

    return `${p.length}|${first.toString()}|${Point.DIMENSIONS}|${shape.count()}|${nearest()}`;
}
"#;

guestjs::guest_module! {
    module Demo {
        fn run() -> String;
    }
}

#[derive(Debug, Clone)]
struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn length_of(&self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
}

#[guestjs::host_class]
impl Point {
    #[guestjs(constructor)]
    fn new(
        x: f64,
        y: f64,
    ) -> Result<Self, Error> {
        Ok(
            Self {
                x,
                y,
            },
        )
    }

    #[guestjs(get)]
    fn x(&self) -> Result<f64, Error> {
        Ok(self.x)
    }

    #[guestjs(set, name = "x")]
    fn set_x(&mut self, value: f64) -> Result<(), Error> {
        self.x = value;

        Ok(())
    }

    #[guestjs(get)]
    fn y(&self) -> Result<f64, Error> {
        Ok(self.y)
    }

    #[guestjs(set, name = "y")]
    fn set_y(&mut self, value: f64) -> Result<(), Error> {
        self.y = value;

        Ok(())
    }

    #[guestjs(get)]
    fn length(&self) -> Result<f64, Error> {
        Ok((self.x * self.x + self.y * self.y).sqrt())
    }

    #[guestjs(method, name = "toString")]
    fn to_string(&self) -> Result<String, Error> {
        Ok(format!("({}, {})", self.x, self.y))
    }

    #[guestjs(static_method)]
    fn origin() -> Result<Point, Error> {
        Ok(
            Point {
                x: 0.0,
                y: 0.0,
            },
        )
    }

    #[guestjs(constant)]
    const DIMENSIONS: i32 = 2;
}

#[derive(Debug, Clone)]
struct Shape {
    points: Vec<Point>,
}

#[guestjs::host_class]
impl Shape {
    #[guestjs(constructor)]
    fn new(
        #[guestjs(detached)] points: Vec<Point>,
    ) -> Result<Self, Error> {
        Ok(Self { points })
    }

    #[guestjs(get)]
    fn points(&self) -> Result<Vec<Point>, Error> {
        Ok(self.points.clone())
    }

    #[guestjs(method)]
    fn count(&self) -> Result<usize, Error> {
        Ok(self.points.len())
    }

    #[guestjs(method)]
    fn nearest(
        &self,
        #[guestjs(borrow)] probe: &Point,
    ) -> Result<HostFn, Error> {
        let target = (probe.x, probe.y);
        let points = self.points.clone();

        Ok(
            HostFn::new(move |_scope, _args| {
                Ok(
                    points
                        .iter()
                        .cloned()
                        .min_by(|a, b| {
                            ((a.x - target.0).powi(2)
                                + (a.y - target.1).powi(2))
                                .total_cmp(
                                    &((b.x - target.0).powi(2)
                                        + (b.y - target.1).powi(2)),
                                )
                        })
                        .map(|point| {
                            format!("({}, {})", point.x, point.y)
                        })
                        .unwrap_or_default(),
                )
            }),
        )
    }
}

struct Geometry;

#[guestjs::host_module(
    name = "host:geometry",
    classes(Point, Shape),
)]
impl Geometry {}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let guest = Runtime::builder()
        .build()
        .await?
        .guest()
        .bind(Geometry)
        .build()
        .await?;

    let module = Demo::from(
        guest
            .guest_module("demo.ts", GUEST_MODULE)
            .await?,
    );

    println!(
        "demo.run() = {:#?}",
        module.run().await?,
    );

    println!(
        "demo.run() = {:#?}",
        guest
            .scope(async move |scope| {
                module.bind(&scope)?
                    .run()
            })
            .await?
    );

    println!(
        "Class::of::<Point>() length = {:#?}",
        Class::of::<Point>(&guest)
            .await?
            .construct((3.0, 4.0))
            .await?
            .borrow_with(|point| point.length_of())
            .await?,
    );

    Ok(())
}
