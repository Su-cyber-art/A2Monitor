use anyhow::{bail, Context, Result};
use monitor_protocol::{Probe, ProbeKind, ProbeResult};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{lookup_host, TcpStream},
};

pub async fn run(probe: &Probe) -> ProbeResult {
    let start = Instant::now();
    let mut result = ProbeResult {
        task_id: probe.id.clone(),
        time: monitor_protocol::now_ms(),
        family: probe.family,
        ..Default::default()
    };
    let outcome = tokio::time::timeout(
        Duration::from_millis(probe.timeout_ms),
        measure(probe, &mut result),
    )
    .await;
    match outcome {
        Ok(Ok(())) => {
            result.success = true;
            result.latency_ms = Some(start.elapsed().as_secs_f64() * 1000.0);
        }
        Ok(Err(e)) => result.error = Some(e.to_string().chars().take(200).collect()),
        Err(_) => result.error = Some("探测超时".into()),
    };
    result
}
async fn measure(probe: &Probe, result: &mut ProbeResult) -> Result<()> {
    let is_http = probe.kind == ProbeKind::Http;
    let parsed = if is_http {
        Some(url::Url::parse(&probe.target)?)
    } else {
        None
    };
    if parsed.as_ref().is_some_and(|u| {
        !matches!(u.scheme(), "http" | "https")
            || !u.username().is_empty()
            || u.password().is_some()
    }) {
        bail!("HTTP 目标只能使用无凭证的 http/https URL");
    }
    let (host, port) = if let Some(url) = &parsed {
        (
            url.host_str()
                .context("缺少主机名")?
                .trim_matches(['[', ']'])
                .to_owned(),
            url.port_or_known_default().context("缺少端口")?,
        )
    } else if probe.kind == ProbeKind::Dns {
        (probe.target.trim_matches(['[', ']']).to_owned(), 443)
    } else {
        let (host, port) = probe
            .target
            .rsplit_once(':')
            .context("目标需要 host:port")?;
        (
            host.trim_matches(['[', ']']).to_owned(),
            port.parse::<u16>()?,
        )
    };
    let dns = Instant::now();
    let addresses: Vec<_> = lookup_host((host.as_str(), port))
        .await?
        .filter(|a| match probe.family {
            4 => a.is_ipv4(),
            6 => a.is_ipv6(),
            _ => true,
        })
        .take(8)
        .collect();
    result.dns_ms = Some(dns.elapsed().as_secs_f64() * 1000.0);
    if addresses.is_empty() {
        bail!("指定地址族没有可用地址");
    }
    if probe.kind == ProbeKind::Dns {
        return Ok(());
    }
    let tcp = Instant::now();
    let mut stream = None;
    for address in addresses {
        if let Ok(Ok(s)) = tokio::time::timeout(
            Duration::from_millis((probe.timeout_ms / 3).max(100)),
            TcpStream::connect(address),
        )
        .await
        {
            result.family = if address.is_ipv4() { 4 } else { 6 };
            stream = Some(s);
            break;
        }
    }
    let stream = stream.context("TCP 连接失败")?;
    result.tcp_ms = Some(tcp.elapsed().as_secs_f64() * 1000.0);
    if probe.kind == ProbeKind::Tcp {
        return Ok(());
    }
    let tls =
        probe.kind == ProbeKind::Tls || parsed.as_ref().is_some_and(|u| u.scheme() == "https");
    if tls {
        let roots =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
        let begin = Instant::now();
        let stream = connector
            .connect(
                rustls::pki_types::ServerName::try_from(host.clone())?,
                stream,
            )
            .await?;
        result.tls_ms = Some(begin.elapsed().as_secs_f64() * 1000.0);
        if is_http {
            http(stream, parsed.as_ref().unwrap(), result).await?;
        }
    } else if is_http {
        http(stream, parsed.as_ref().unwrap(), result).await?;
    }
    Ok(())
}
async fn http<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    url: &url::Url,
    result: &mut ProbeResult,
) -> Result<()> {
    let path = format!(
        "{}{}",
        if url.path().is_empty() {
            "/"
        } else {
            url.path()
        },
        url.query().map(|q| format!("?{q}")).unwrap_or_default()
    );
    let host = url.host_str().context("缺少主机名")?;
    let header = if let Some(port) = url.port() {
        format!("{host}:{port}")
    } else {
        host.to_owned()
    };
    let request=format!("GET {path} HTTP/1.1\r\nHost: {header}\r\nUser-Agent: Monitor-Probe/0.1\r\nConnection: close\r\nAccept: */*\r\n\r\n");
    let begin = Instant::now();
    stream.write_all(request.as_bytes()).await?;
    let mut buffer = [0u8; 1024];
    let n = stream.read(&mut buffer).await?;
    result.ttfb_ms = Some(begin.elapsed().as_secs_f64() * 1000.0);
    anyhow::ensure!(n > 0, "空的 HTTP 响应");
    let mut head = buffer[..n].to_vec();
    while !head.contains(&b'\n') && head.len() < 8192 {
        let n = stream.read(&mut buffer).await?;
        anyhow::ensure!(n > 0, "HTTP 状态行不完整");
        head.extend_from_slice(&buffer[..n]);
    }
    anyhow::ensure!(head.contains(&b'\n'), "HTTP 状态行过长");
    let line_end = head.iter().position(|b| *b == b'\n').unwrap();
    let first = std::str::from_utf8(&head[..line_end])?
        .lines()
        .next()
        .context("空的 HTTP 响应")?;
    result.status_code = first.split_whitespace().nth(1).and_then(|s| s.parse().ok());
    anyhow::ensure!(first.starts_with("HTTP/1."), "无效 HTTP 协议");
    match result.status_code {
        Some(code) if (200..400).contains(&code) => Ok(()),
        Some(code) => bail!("HTTP {code}"),
        None => bail!("无效 HTTP 响应"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn http_probe_records_connection_and_first_byte_stages() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut b = [0; 1024];
            assert!(s.read(&mut b).await.unwrap() > 0);
            s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK")
                .await
                .unwrap();
        });
        let p = Probe {
            id: "test".into(),
            name: "test".into(),
            kind: ProbeKind::Http,
            target: format!("http://{address}/"),
            family: 4,
            ..Default::default()
        };
        let r = run(&p).await;
        assert!(r.success, "{:?}", r.error);
        assert_eq!(r.status_code, Some(200));
        assert!(r.tcp_ms.is_some() && r.ttfb_ms.is_some() && r.tls_ms.is_none());
    }
    #[tokio::test]
    async fn fragmented_headers_and_binary_bodies_do_not_fail_http_probes() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 1024];
            assert!(socket.read(&mut request).await.unwrap() > 0);
            socket.write_all(b"H").await.unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
            socket
                .write_all(b"TTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\n\xff")
                .await
                .unwrap();
        });
        let result = run(&Probe {
            id: "fragmented".into(),
            name: "fragmented".into(),
            kind: ProbeKind::Http,
            target: format!("http://{address}/"),
            ..Default::default()
        })
        .await;
        assert!(result.success, "{:?}", result.error);
        assert_eq!(result.status_code, Some(200));
    }
}
