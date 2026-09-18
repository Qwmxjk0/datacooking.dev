# DataCooking.dev

เว็บเครื่องมือแปลงไฟล์ ใช้ฟรี ไม่ต้องสมัคร  
ไซต์จริง: [https://datacooking.dev](https://datacooking.dev)

ทำขึ้นให้คนไทยใช้ง่าย โดยเฉพาะงานออฟฟิศที่เปิดไฟล์แล้วตัวอักษรเพี้ยน หรืออยากทำให้ไฟล์เล็กลง

## เครื่องมือ

**งานไฟล์**
- แก้ไฟล์เพี้ยน — CSV/TXT ที่เปิดใน Excel แล้วไทยอ่านไม่ได้
- ทำให้ไฟล์เล็กลง — แปลง CSV ↔ Parquet

**ของเล่นเน็ต**
- ดู IP ของเครื่องที่ใช้อยู่
- คำนวณ subnet เช่น `192.168.1.0/24`

**คุยบนเซิร์ฟเวอร์**
- MiniCPM5-1B (Q4) รันบน Contabo ไม่ได้รันบนเครื่องผู้ใช้

**อื่นๆ**
- ดู RAM/CPU ของเซิร์ฟเวอร์ (แถบหัวเว็บ)
- คิวกำลังใจ — ทักทายสั้นๆ ได้ ไม่รับเงิน

อัปโหลดสูงสุด 100 MB ต่อไฟล์ แล้วลบทิ้งหลังจบคำขอ

## รันบนเครื่อง

ต้องมี Rust

```bash
cargo run
```

เปิด [http://localhost:3000](http://localhost:3000)

แก้หน้าเว็บใน `static/` แล้วรีเฟรชได้เลย ไม่ต้อง build ใหม่

```bash
cargo test
```

## Deploy

Push ขึ้น `main` แล้ว GitHub Actions จะเทส, build รูป Docker, แล้ว SSH ไปเซิร์ฟเวอร์

Environment บน GitHub ชื่อ `SSH_PASS`

| ชนิด | ชื่อ | ความหมาย |
|---|---|---|
| Secret | `SECRET` | รหัส SSH ของ `root` |
| Variable (ไม่บังคับ) | `DEPLOY_HOST` | ค่าเริ่มต้น `109.205.178.227` |
| Variable (ไม่บังคับ) | `DEPLOY_USER` | ค่าเริ่มต้น `root` |
| Variable (ไม่บังคับ) | `DOMAIN` | ค่าเริ่มต้น `datacooking.dev` |

โฟลเดอร์บนเซิร์ฟคือ `/opt/datacooking`

DNS ของโดเมนต้องเป็น A record `@` ชี้ IP เซิร์ฟ และปิด Cloudflare Proxy (เมฆเทา) จนกว่าใบ HTTPS จะออก

## ตัวแปรแวดล้อม

| ชื่อ | ค่าเริ่มต้น | ความหมาย |
|---|---|---|
| `PORT` | `3000` | พอร์ตเว็บ |
| `STATIC_DIR` | `static` | โฟลเดอร์หน้าเว็บ |
| `DATA_DIR` | `data` | ที่เก็บคิวกำลังใจ |
| `LLM_URL` | ว่าง | ที่อยู่ llama-server บนเซิร์ฟ เช่น `http://llm:8080` |

โมเดล MiniCPM5-1B Q4 ถูกดาวน์โหลดบนเซิร์ฟเวอร์ตอน `docker compose up` ครั้งแรก (~688 MB, inbound ไม่คิดเงิน)

## ผู้ทำ

[Kittanai Kaptaphon](https://github.com/Qwmxjk0) · [LinkedIn](https://www.linkedin.com/in/kittanai-kaptaphon/)
