#!/usr/bin/env sh

function die {
    kill $(jobs -p) > /dev/null 2>&1
    rm -f index_tls.html
    exit $1
}

if [ ! -e weylus.pem ]
then
    openssl req -batch -newkey rsa:4096 -sha256 -keyout weylus.key -nodes -x509 -days 365 \
        -subj="/CN=Weylus" -out weylus.crt

    cat weylus.key weylus.crt > weylus.pem
    rm weylus.key weylus.crt
fi

if [ -z "$WEYLUS" ]
then
    if [ -e weylus ]
    then
        WEYLUS=./weylus
    else
        if which weylus > /dev/null 2>&1
        then
            WEYLUS=weylus
        else
            echo "Please specify path to weylus."
            echo -n "> "
            read -r WEYLUS
        fi
    fi
fi

trap die SIGINT

$WEYLUS --print-index-html | sed 's/{{websocket_port}}/9001/' > index_tls.html
$WEYLUS --custom-index-html index_tls.html \
    --bind-address 127.0.0.1 \
    --web-port 1702 \
    --websocket-port 9002 \
    --no-gui &

hitch --frontend=[0.0.0.0]:1701 --backend=[127.0.0.1]:1702 \
    --daemon=off --tls-protos="TLSv1.2 TLSv1.3" weylus.pem &

hitch --frontend=[0.0.0.0]:9001 --backend=[127.0.0.1]:9002 \
    --daemon=off --tls-protos="TLSv1.2 TLSv1.3" weylus.pem &

wait
